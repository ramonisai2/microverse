# Plan — Picking por click en la escena 3D del editor

**Estado:** implementado (PC). **Creado:** 2026-09-27.
**Regla:** este documento es el diseño y la nota de lo implementado. No se
mezcla con `docs/plan_fase6.md` (que es el diseño de la timeline); esto es un
hito posterior, ya cerrado.

---

## 1. El problema

La selección de entidad en el editor de voxels era **solo ciclado**: `SelPrev` /
`SelNext` (`O` / `P`) recorren la lista del panel. Con 20 entidades apiladas,
elegir una exige `P` × 19 y el panel solo muestra `EDITOR_VISIBLE_ROWS = 5`
(`hud.rs`), así que ni siquiera se ven las que no están en la ventana.

El click sobre la entidad en la escena 3D no existía.

---

## 2. Qué se reutiliza (leer antes de tocar código)

| Pieza | Dónde | Nota |
|---|---|---|
| `Camera::screen_ray(x, y, lw, lh) -> (Vec3, Vec3)` | `src/camera.rs:495` | Ya existía **para el minado apuntado** del juego (`lib.rs`, `tap_at` y `press_mine_aimed`). Puro y testeado: el centro devuelve `forward()`. |
| `EditorEntity::aabb() -> (Vec3, Vec3)` | `src/editor.rs:286` | AABB en mundo con `scale → skew → rotation` ya aplicados (vía `corners()`). Es **la misma geometría que dibuja los marcadores** (`marker_cells`, `lib.rs`). |
| `App::logical_size()` | `src/lib.rs` | Tamaño en píxeles lógicos, el mismo que usa el hit-test del HUD. |
| `App::mouse_logical` | `src/lib.rs` | Actualizado en `CursorMoved`, también en `Screen::Editor` (en el editor el cursor nunca se captura: HD-2D no lo captura, `sync_cursor_for_ui`). |
| `HudHitRegion` / `hit_test` | `src/hud.rs` | El panel se consulta **antes** que la escena, así que no hay conflicto. |
| `raycast_reach` (DDA de voxels) | `src/world.rs` | **No reutilizable**: devuelve celdas `IVec3` de un rayo contra la grilla. No hay ningún ray-vs-caja en el proyecto. |

**Lo que hay que escribir:** el test de slabs (ray vs AABB) y la resolución del
ciclo. Nada de readback de GPU: en el editor las entidades no son instancias
con índice, son celdas de voxel resaltadas más el mesh de preview, así que el
picking es **analítico en CPU**.

---

## 3. Decisiones (confirmadas por el usuario 2026-09-27)

1. **Nearest-hit + ciclo en clicks sucesivos.** El click elige la entidad con el
   `t` de impacto más cercano a cámara; un segundo click en el mismo sitio
   (±4 px, ≤ 400 ms) rota al siguiente candidato en orden de profundidad.
   *Por qué las dos y no solo una:* `Add` crea la entidad en el `focus` y
   `Duplicate` la sube por `size[1]`, así que el solapamiento es el caso
   normal, no el raro. Solo "la más cercana" deja la entidad de atrás
   inaccesible sin mover la cámara.
2. **Se descarta "mayor prioridad"** como criterio: `EditorEntity` no tiene
   campo de prioridad y `kind_color()` es lo único que ordena visualmente.
   Implementarlo sería inventar z-order en el modelo de datos.
3. **Convive con `SelPrev`/`SelNext`**, no los reemplaza. No hay razón técnica
   en contra: son `HistoryEffect::None` (no tocan el documento), el final del
   código es el mismo (`selected` + `clamp_scroll` + `clamp_state_sel`) y
   `O`/`P` quedan como atajo. La lista del panel sigue siendo el índice de
   "qué hay en la escena", útil justo cuando hay solapamiento.
4. **La selección entra como `EditorAction::Pick(usize)`**, no escribiendo
   `editor.selected` desde el handler de ratón: `apply_hud_action` es el único
   camino de entrada y `history_effect` sigue siendo la fuente de verdad.
5. **Solo PC.** `tap_at` corta con `screen != Screen::Playing`, así que un tap
   en la escena del editor en Android se descarta hoy; no se toca (queda como
   hito aparte si el móvil lo necesita).

---

## 4. Lo implementado

### 4.1 `src/editor.rs` — `EditorEntity::ray_hit`

Test de slabs contra la AABB propia, eje a eje:

- Devuelve la distancia del **primer** impacto, o `None`.
- Un eje con `dir` casi 0 (paralelo) con el origen dentro del intervalo no acota
  `t`; con el origen fuera, descarta la entidad. Se evita la división por
  `1/dir` a propósito: `0 * inf` es `NaN` y envenena las comparación.
- Caja no finita (escala 0 en un eje, datos rotos) → `None`, igual que
  `cover_cells`.

### 4.2 `src/lib.rs` — memoria del ciclo y resolución

- `ED_PICK_RANGE` = `ED_MARKER_RANGE` (72): es el rango con el que se dibujan
  los marcadores, así que el radio de elección no puede ser una constante
  nueva que se desincronice. **Aproximación conocida:** el filtro es por
  distancia de impacto, mientras que el dibujado filtra por distancia del
  *centro*; una caja enorme con el centro fuera de rango pero el borde dentro
  se puede elegir antes de dibujarse. El budget `ED_MAX_MARKER_CELLS` ya hace
  que "dibujado" sea aproximado, así que la invariancia no existe hoy.
- `PickCycle { px, py, at, slot, entity }` en `EditorState::last_pick`: **memoria
  de UI, no del documento**, así que va fuera de `EditorSnapshot` (igual que
  `scroll`, `panel` o `load_cursor`, que tampoco están).
- `pick_candidates()` ordena por profundidad con **desempate por índice**, para
  que dos entidades empatadas no cambien de lugar entre frames.
- `pick_at()` avanza de slot solo si se cumple **todo**: el click anterior está
  a ≤ 4 px, hace ≤ 400 ms, y el slot anterior **sigue ocupando la misma entidad**
  (si la lista cambió, el índice guardado apuntaría a otra y el clic cicla desde
  el primero). El tiempo viene de `App::last_frame` (`Instant`, monótono) y se
  pasa como `f32` para que la función siga siendo pura y testeable.
- `restore()` (Undo/Redo) **invalida la memoria del ciclo**: el documento puede
  haber cambiado de forma que el slot guardado ya no signifique lo mismo.
- `apply(Pick(i))` valida el índice: un índice fuera de rango (escena vacía, o
  un click enlatado) no toca nada. Sin `status`: el feedback es la fila del panel
  y el marcador blanco, igual que con `SelPrev`/`SelNext`.

### 4.3 `src/lib.rs` — el clic

`WindowEvent::MouseInput`, en el brazo `Screen::Editor` que antes hacía
`return` a secas: el hit-test del panel ya se consultó antes, así que un clic que
llega aquí es, por definición, un clic en la escena. Solo `MouseButton::Left`:
el derecho se deja libre.

### 4.4 `src/hud.rs` — la acción

`EditorAction::Pick(usize)` con el índice de entidad, junto a `SelPrev` /
`SelNext`. Al ser una `HudAction::Ed` normal, la acción no la produce ningún
botón: la emite el handler de ratón.

### 4.5 Tests

- `editor.rs` (unit): `ray_hit` acierta desde fuera, falla al lado, acierta con
  origen dentro, no acota con dirección paralela, `None` con caja no finita.
- `lib.rs` (`hitch_tests`): elige la entidad bajo el cursor; con dos
  solapadas elige la más cercana; dos clicks en el mismo sitio ciclan y el
  tercero vuelve al principio; un click lejos en el sitio o en el tiempo
  reinicia el ciclo; si la lista de candidatos cambió, el ciclo no avanza a un
  slot fantasma; una entidad fuera de rango no se elige; **elegir no genera paso
  de undo** (ni toca el redo); `Undo` limpia la memoria del ciclo; un índice
  inexistente no deselecciona.
- `editor_history_policy_matches_every_action` cubre la variante nueva sin
  tocarlo: su `variant_count()` es un `match` sin `_`, así que **añadir
  `Pick` rompe la compilación** hasta que se añada al recuento (55 → 56) y a
  `all_editor_actions`.

---

## 5. Lo que NO se hizo (y por qué)

- **Táctil / Android:** `tap_at` corta con `screen != Screen::Playing`. Un tap
  en la escena del editor sigue sin hacer nada. Decisión del usuario: solo PC.
- **Reemplazar `SelPrev`/`SelNext`:** no hay razón. Ver decisión 3.
- **Prioridad / z-order:** ver decisión 2.
- **Picking contra el mesh** (los microvoxels del modelo): se usa la caja, que
  es la misma geometría que dibuja el marcador. Es lo que el usuario ve, y es
  O(entidades) en CPU, sin coste de GPU.

---

## 6. Deuda / puntos abiertos

- El radio de picking es el del dibujado, pero los **dos filtros no son el
  mismo** (impacto vs. centro). Ver la aproximación en §4.2.
- El ciclo se apoya en el reloj de frame (`last_frame`), no en el del evento: en
  un frame lento dos clicks pueden parecer más separados de lo que fueron. Es
  un detalle del orden de magnitud (400 ms), no un bug.
- `Pick` no aparece en ningún panel: si algún día se quiere elegir entidad desde
  el teclado con la escena como referencia, sería otra action, no esta.