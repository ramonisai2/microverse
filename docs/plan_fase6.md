# Plan Fase 6 — Timeline de animación (y refinamiento manual de clips)

**Estado:** diseño, sin implementar. **Creado:** 2026-09-26.
**Regla:** este documento es de diseño. Nada aquí está escrito en código, y nada
se implementa hasta que el usuario lo apruebe fase a fase.

---

## 1. Qué existe hoy (referencia al código, leer antes de diseñar)

| Pieza | Dónde | Nota |
|---|---|---|
| `FrameFile { t: f32, pose: BTreeMap<String,f32> }` | `src/animation.rs:71` | El keyframe actual es un **pose**: tiempo + 12 articulaciones en **grados**. |
| 12 articulaciones, **solo ángulos** | `canonical_joint` `animation.rs:160` | `head_y, l_arm_z, r_arm_z, l_arm_x, r_arm_x, l_elbow_x, r_elbow_x, l_leg_x, r_leg_x, l_knee_x, r_knee_x, l_foot_x`. **No hay position, ni scale, ni skew** en `HeroPose`. |
| `HeroPose` en **radianes** | `src/hero_pose.rs:9` | El editor trabaja en **grados** (`ED_ROT_STEP = 15.0`, `lib.rs:107`). Conversión en ambos extremos. |
| `pose_from_map` / `pose_to_map` | `animation.rs:179` / `:206` | **Privadas.** `pose_to_map` solo la usa `bake_clip`. |
| `AnimationClip::from_json_str(&str)` | `animation.rs:119` | **Pública.** Un clip se puede parsear sin el registro. |
| `CLIP_NAMES: [&str; 9]` + `load_registry` + `clip()` | `animation.rs:42`, `:262`, `:297` | `clip()` usa `OnceLock`: lo que hay al arrancar es lo que hay en la sesión. |
| `embedded_clip_json(name)` | `animation.rs:247` | `match` sobre los 9 nombres: **un clip nuevo no se puede embeber**. |
| `clip_search_paths(rel)` | `animation.rs:229` | Raíces candidatas: cwd, exe dir, `CARGO_MANIFEST_DIR`. |
| `Transform` + `transform()/set_transform()` en entidad y estado | `src/editor.rs` | **Costura del patrón**: resolver dueño → sacar → mutar → escribir. |
| `EditorAction` (20 steppers de transform) | `src/hud.rs:65` | `Copy`, discreto, un paso por botón. |
| Selección: `SelPrev/Next` + `EDITOR_VISIBLE_ROWS = 5` | `hud.rs:2128` | Patrón de **lista vertical** con ventana de scroll. |
| **El editor no tiene undo** | `EditorState`, `lib.rs` | Sin pila de deshacer. Requisito nuevo. |
| **Nada lee `EditorScene` en runtime** | (verificado por grep) | El juego no carga escenas. |

---

## 2. Decisiones ya tomadas (no re-litigar)

1. **Un keyframe es una pose completa** (las 12 articulaciones), no un `Transform`.
   El patrón de steppers se reutiliza un nivel más abajo: se selecciona una
   articulación (como hoy se selecciona entidad/estado) y se aplica `Rot±` sobre
   ella.
2. **Requisito a incorporar:** cuando una animación llegue importada (borrador
   tosco de keyframes generado por una IA externa), el usuario debe poder
   **refinarla a mano keyframe por keyframe** en el mismo editor, con el mismo
   patrón de steppers.
3. **Sin bool "revisado".** El estado "sin revisar" se **deriva por diff contra
   la línea base importada** y se muestra como marca visual en el timeline
   (diamante hueco vs. lleno). Un bool puede mentir ("editado y revertido").
4. **Un solo artefacto: el clip.** El import es bulk y produce un clip entero;
   el refinamiento es incremental. Mismos datos, **dos superficies de UI**:
   import en el panel ARCHIVO (es una acción de fichero, junto a `CARGAR`),
   refinamiento en el panel ANIMAR. No se crean dos representaciones.
5. **Undo es bloqueante** para la Fase 6: refinar 40 keyframes a mano sin
   ctrl-Z no es viable. No se diseña todavía.
6. **Composición del transform (Fase 3.2, decided, no implementada):** `scale`
   se aplica **encima** de `size`; la entidad acaba midiendo
   `size[1] * scale[1]`.
7. **Selección a 4 niveles: el patrón se conserva, la mecánica se toca en 3
   puntos** (confirmado 2026-09-26). Diseño completo en §4:
   **target polimórfico** (un escalar de articulación, no un `Transform`),
   **conversión de unidades** (grados ↔ radianes) y **clamps como datos**
   (descriptor por target, no constantes incrustadas en la tabla de steppers).
8. **Alcance de los clips: (C) + (A).** (C) el editor carga/refina/exporta sus
   propios clips con **cero cambios al juego**; (A) es **el único cambio de
   runtime** que se hace (unir `CLIP_NAMES` con un `read_dir`, conservando los
   9 embebidos de respaldo). **(B) queda fuera de esta fase**, como hito
   aparte. Detalle y limitaciones en §5.

---

## 3. Abierto — puntos pendientes de resolver antes de implementar

### 3.1 ~~¿Cuatro niveles de selección escalan?~~ → RESUELTO

Ver decisión 7 en §2 y el diseño en §4.

### 3.2 ~~¿Qué cambio mínimo hace que `load_registry` reconoce clips?~~ → RESUELTO

Ver decisión 8 en §2 y el diseño + limitaciones en §5.

### 3.3 Otros puntos anotados (siguen abiertos)

- **Agrupar el estado de UI de la editor** (`EditorView { panel, selected,
  scroll, state_sel, kf_sel, clip }`) antes de añadir el cuarto nivel de
  selección. Cada nivel nuevo hoy se paga con un parámetro más en
  `build_editor_hud` (ya va por 9). No es bloqueante, pero se paga 4 veces.
- **`SizeDec/SizeInc` y `Skew*` en keyframes:** no tienen destino (una
  articulación no tiene caja ni cizalla). Decidir: inertes, o einziges en el
  panel ANIMAR.
- **Línea base del diff:** vive en memoria (no en el fichero) → al reabrir la
  escena se pierden las marcas de "sin revisar". Si deben sobrevivir, pasan al
  fichero, y eso es decisión del usuario.

---

## 4. Análisis: 4 niveles de selección (entidad → estado → keyframe → articulación)

**Selección: escala casi gratis.** `EditorState` ya tiene `selected`, `scroll`,
`state_sel`. Añadir `kf_sel` y `joint_sel` son dos campos más y dos
`clamp_*()`. El `enum EditorAction` se extiende con `StateSel(usize)` como precedente:
`KeyframeSel(usize)` y `JointSel(usize)` son variants nuevas, sin tocar las
existentes.

**La costura de lectura-mutación-escritura aguanta**, pero **la tabla de steppers NO
escala sin cambios.** Motivos concretos:

1. **El target deja de ser un `Transform` y pasa a ser un escalar.** Una
   articulación es **un `f32`** dentro de `HeroPose`, no un struct de 4 canales.
   `HeroPose` no es un array indexable: son 12 campos con nombre. Hace falta un
   `match` campo↔índice, o un viaje por `BTreeMap<String,f32>` con
   `pose_from_map`/`pose_to_map`, que hoy son **privadas**.
2. **Unidades:** el keyframe guarda grados; `HeroPose` guarda radianes.
   Conversión en la entrada y en la salida.
3. **De los 20 steppers, en una articulación solo tienen sentido los 6 de
   `Rot*`**, y por articulación solo 1-2 ejes (`head_y` es Y; `l_arm_x` es X).
   `Pos*`, `Scl*`, `Size*` y `Skew*` **no existen** para un ángulo de
   articulación. El panel ANIMAR mostraría un subconjunto, no "los mismos
   steppers un nivel más abajo".
4. **Clamps por tipo de target:** `ED_POS_LIMIT = 512` significa "lejos del
   spawn" y no tiene sentido para un keyframe; un ángulo de articulación tiene
   otro rango natural. Confirma el punto 2 de las notas originales: **pasos y
   clamps tienen que pasar a ser datos del target**, no constantes incrustadas
   en la tabla (`bump(&mut t.position[0], -ED_POS_STEP, -ED_POS_LIMIT, ED_POS_LIMIT)`).
5. **El widget de lista no se reutiliza.** `SelPrev/Next` + scroll es una
   **lista vertical**; un timeline es un **eje temporal horizontal** con
   diamantes. Las acciones de selección escalan; el widget es nuevo.

**Veredicto (CONFIRMADO 2026-09-26):** el *patrón* (stepper discreto + target
seleccionado + read-modify-write) es lo correcto y se conserva. La *mecánica*
se toca en tres puntos, confirmados como diseño de la Fase 6:

1. **Target polimórfico** — `Transform` (entidad/estado) | escalar de
   articulación (keyframe) | nada (canales sin destino). Se implementa como un
   descriptor por target, no como una cadena de `if`.
2. **Conversión de unidades** — grados (keyframe, `FrameFile.pose`) ↔ radianes
   (`HeroPose`) en la entrada y la salida del read-modify-write.
3. **Clamps como datos** — pasos y rangos (`ED_POS_STEP`, `ED_POS_LIMIT`,
   `ED_ROT_STEP`, `ED_SCALE_RANGE`, `ED_SKEW_LIMIT`…) salen de la tabla y pasan
   al descriptor del target. Fin de las constantes incrustadas en los arms.

**No es una reescritura; es mediano.** Nota de alcance: en el panel ANIMAR solo
se muestran los steppers con significado en el target activo (§4, punto 3), así
que la UI será un subconjunto de los 20 de hoy, no la parrilla completa.

---

## 5. Alcance de los clips — DECIDIDO: (C) + (A)

`load_registry` itera `CLIP_NAMES` (9 nombres fijos) y `embedded_clip_json` es
un `match` sobre esos mismos 9. Hay tres alcances distintos, de menos a más:

### (C) El editor ve y refina sus propios clips — **cero cambios** ✅ ADOPTADO

`AnimationClip::from_json_str(&str)` ya es **pública** (`animation.rs:119`).
El editor puede cargar su clip a memoria, muestrearlo con `sample(t)` y
guardarlo con `ClipFile`. **No toca el registro en absoluto.** Es el bucle de
autoría de la Fase 6: importar → refinar a mano → exportar.

### (A) El juego reconoce clips del editor por nombre, sin editar `CLIP_NAMES` — **pequeño** ✅ ADOPTADO

Unión de `CLIP_NAMES` con un `read_dir` de `assets/animations/*.json` sobre las
raíces que ya produce `clip_search_paths`, conservando los 9 nombres como
respaldo embebido.

- **Tamaño:** pequeño, ~30-50 líneas.
- **Archivos:** `src/animation.rs` únicamente (+ test en el mismo archivo).
- **Trampas:** (1) `OnceLock` ⇒ hace falta **reiniciar** para ver un clip
  nuevo; (2) el editor durante la sesión seguiría sin verlo en el juego hasta
  reiniciar (irrelevante si el editor usa (C)).

#### LIMITACIÓN CONOCIDA (Android/APK) — no se resuelve en esta fase

En Android los clips van **embebidos** con `include_str!` en
`embedded_clip_json` (`animation.rs:247`), que es un `match` sobre los 9
nombres fijos. (A) **no resuelve el móvil**: un clip nuevo escrito por el editor
**no llega al APK** sin regenerar el embebido y **recompilar y volver a
empaquetar**. Es una limitación asumida, no un bug a arreglar aquí. En el móvil,
el bucle de autoría (C) funciona para editar y exportar, pero lo exportado solo
se plays si el dispositivo tiene los assets en disco.

### (B) El juego juega lo que la escena del editor referencia — **grande** ❌ FUERA DE FASE

Requiere un cargador de escenas en runtime (**no existe**: nada lee
`EditorScene` fuera del editor), resolución de nombres de clip, y decidir
cuándo se carga. Es integración de runtime, no un cambio de registro.
**Hito aparte, posterior a la Fase 6.**

**Alcance cerrado:** (C) + (A) para la Fase 6; (B) para otro hito.

---

## 6. Deuda técnica conocida (no bloqueante)

- **Caché de modelos duplicada por grafía:** `preview_model` indexa por la
  cadena *pedida*, así que `"hero"` y `"assets/entities/hero.json"` son dos
  claves y cargan dos copias. Anotado en el doc comment de
  `entity_model::preview_model`. No arreglar salvo que lo pida; el arreglo
  exigiría que `load_entity_from_disk` devuelva la ruta que resolvió, lo que
  toca los tres cargadores existentes.
