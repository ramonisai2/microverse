# Manual — Casas y construcciones de aldeas y centros

Guía para diseñar, editar y publicar **prefabs** (casas, torres, tiendas, muros
decorados…) que las aldeas y capitales del mundo instancian automáticamente.

- **Editor visual:** `editor/prefab.html` (abrir en el navegador).
- **Prefabs del juego:** `assets/prefabs/*.json`.
- **Colocación en el mundo:** `src/settlements.rs` + `src/prefab.rs`.

---

## 1. Conceptos en 30 segundos

| Término | Qué es |
|---|---|
| **Prefab** | Una construcción, guardada como JSON de capas de voxels. |
| **Huevo (egg)** | Un marcador que el plano de un asentamiento reserva en un lote; al generarse el mundo, "eclosiona" e instancia el prefab. |
| **Anchor** | Punto de referencia del prefab. Se coloca sobre la superficie del terreno. |
| **Lote (lot)** | Espacio que el huevo reserva; se limpia de hierba/árboles antes de estampar. |
| **Rotación** | El huevo puede rotar la casa en pasos de 90° (según semilla). |

Las **casas no se hornean a mano** en la generación del mundo: el asentamiento
solo reserva el lote (huevo) y apunta a un archivo de prefab. Esto permite
rediseñar construcciones sin tocar la generación de terreno.

---

## 2. Usar el editor visual (`editor/prefab.html`)

Abrí el archivo en un navegador (o desde `editor/index.html` → tarjeta de
prefabs). Al cargar aparece la **casa básica** de ejemplo.

### Paneles

1. **Prefab** (izquierda): ID, nombre, tamaño `X/Y/Z` y tamaño de lote.
2. **Material**: paleta de pintura (ver §3).
3. **Planta por capa** (centro): rejilla cenital de la capa `Y` actual.
4. **Vista isométrica** (derecha): previsualización 3D en vivo.

### Controles

| Acción | Cómo |
|---|---|
| Pintar celda | **Clic izquierdo** sobre la rejilla |
| Borrar celda (→ aire) | **Clic derecho** |
| Cambiar de capa `Y` | **Rueda del ratón** o el deslizador *Capa* |
| Elegir material | Clic en un botón de la paleta |
| Redimensionar | Ajustá `X/Y/Z` y pulsá **Redimensionar conservando** |
| Vaciar / Rellenar capa | Botones **Vaciar capa** / **Rellenar capa** |
| Guardar | **Exportar JSON** (descarga el archivo) |
| Abrir | **Importar JSON** |

Guías visuales: el **recuadro blanco** en la rejilla marca el *anchor*; la
**flecha amarilla** (visible en la capa `Y=1`) marca la **entrada sur**. En la
preview, el **punto blanco** es el anchor y la etiqueta "ENTRADA SUR" indica
hacia dónde mira la puerta antes de rotar.

### Convenciones de diseño

- **Capa `Y = 0` = cimiento** — normalmente piedra maciza; se apoya en el suelo.
- **Entrada al sur** (`-Z`): dejá un hueco de puerta en esa pared para que el
  huevo la alinee con el camino/plaza. Al rotar el huevo, la entrada girará con
  la casa.
- **Techo arriba**: usá `wood_roof` en la capa superior.
- Mantené el prefab dentro del lote (deja 1 celda de margen a cada lado).

---

## 3. Paleta de materiales

| Símbolo | Editor | Nombre JSON | Material en el juego | Notas |
|:---:|---|---|---|---|
| `.` | Aire | `air` | — | Vacío (no se estampa) |
| `S` | Piedra aldea | `village_stone` | `VillageStone` | Gris bajo y cálido/amarillento para cimientos y parches |
| — | Cobblestone | `cobble` / `cobblestone` | `Cobblestone` | Murallas de capital/aldea (estampado automático) |
| `W` | Tablones | `wood_planks` | `WoodPlanks` | Tablas aserradas; no usa textura de tronco |
| `G` | Vidrio | `glass` | `Glass` | Ventanas (tinte celeste, se excava como madera) |
| `R` | Techo madera | `wood_roof` | `WoodPlanks` | Alias de tablones para tejados |
| `D` | Tierra | `dirt` | `Dirt` | Jardines, terraplenes |

> Materiales desconocidos en el JSON se **ignoran** con una advertencia. Para
> añadir uno nuevo hay que ampliar `material_from_name` en `src/prefab.rs` (y el
> `enum Material` en `src/world.rs` si es un material inédito).

---

## 4. El formato JSON

Ejemplo real: `assets/prefabs/house_basic.json`.

```json
{
  "schema_version": 1,
  "id": "house_basic",
  "name": "Casa básica",
  "size": [7, 5, 7],
  "anchor": [3, 0, 3],
  "entrance": { "position": [3, 1, 0], "facing": "south" },
  "lot": { "width": 9, "depth": 9 },
  "palette": { ".": "air", "S": "village_stone", "W": "wood_planks", "G": "glass", "R": "wood_roof" },
  "layers": [
    ["SSSSSSS", "SSSSSSS", "SSSSSSS", "SSSSSSS", "SSSSSSS", "SSSSSSS", "SSSSSSS"],
    ["WWW.WWW", "W.....W", "W.....W", "W.....W", "W.....W", "W.....W", "WWWWWWW"],
    ...
  ]
}
```

| Campo | Significado |
|---|---|
| `schema_version` | Versión de formato (actual: `1`). |
| `id` / `name` | Identificador y nombre legible. |
| `size` | `[X, Y, Z]` en voxels. |
| `anchor` | `[ax, ay, az]`: celda que se ancla a la superficie del terreno. |
| `entrance` | Guía de orientación (posición + `facing`). Referencia para diseño. |
| `lot` | `width`/`depth`: huella reservada alrededor del prefab. |
| `palette` | Mapa símbolo → nombre de material. |
| `layers` | Lista de capas, **una por nivel `Y`** (de abajo hacia arriba). |

### Reglas de las capas

- `layers.len()` debe ser **igual a `size.y`**.
- Cada capa tiene **`size.z` filas**; cada fila tiene **`size.x` caracteres**.
- Índices: `layers[y][z][x]`. El eje `+Z` avanza hacia el sur→norte según cómo lo
  leas; en el editor, la fila superior de la rejilla es `z = 0`.
- El validador rechaza tamaños que no cuadren (verás el error en el `status`).

### Cómo se coloca en el mundo (anchor + altura)

Al eclosionar el huevo en `(egg_x, egg_z)`:

1. Se toma la altura del terreno `h = terrain_height(egg_x, egg_z)`.
2. El **anchor** del prefab se coloca en `(egg_x, h, egg_z)`.
3. Cada celda `(lx, ly, lz)` se coloca en:
   `world = anchor_world + rotar_xz(lx - ax, lz - az) + (0, ly - ay, 0)`.

Es decir: con `anchor = [w/2, 0, d/2]`, la casa queda **centrada** en el huevo y
su **capa `Y=0` se apoya sobre la superficie**.

### Rotación

El huevo elige `rotation_quarters` ∈ `{0,1,2,3}` (0°, 90°, 180°, 270° en sentido
horario visto desde arriba). La rotación se aplica solo en el plano `XZ`; la
altura no cambia. Diseñá con la **entrada al sur** y el sistema la orientará.

---

## 5. Publicar un prefab en el juego

### A) Reemplazar la casa por defecto

Todas las aldeas y capitales instancian hoy `assets/prefabs/house_basic.json`
(constante `DEFAULT_HOUSE_PREFAB` en `src/settlements.rs`).

- Exportá desde el editor con el **mismo nombre** `house_basic.json` y
  reemplazá el archivo en `assets/prefabs/`.
- Nota: `house_basic.json` está **embebido en el binario** (`include_str!` en
  `src/prefab.rs`), así que cambios en ese archivo requieren **recompilar**
  (`cargo run`). Otros prefabs se leen de disco en tiempo de ejecución.

### B) Añadir un prefab nuevo (tienda, torre, etc.)

1. Diseñalo en el editor y **Exportar JSON** → guardalo en
   `assets/prefabs/mi_torre.json`.
2. Apuntá un huevo a ese archivo en `src/settlements.rs`, en `settlement_plan`,
   donde se crean los `FeatureKind::PrefabEgg`:

```rust
FeatureKind::PrefabEgg {
    prefab: "assets/prefabs/mi_torre.json",
    rotation_quarters: ((jitter >> 8) & 3) as u8,
}
```

3. `load_prefab` cachea por ruta y lee de disco cualquier archivo que no sea el
   `house_basic` embebido, así que no hace falta recompilar para *editar* ese
   JSON (sí para cambiar a qué archivo apunta el código).

> Para mezclar varios prefabs (p. ej. capital = mezcla de casas, torre y tienda)
> se puede elegir la ruta por índice de slot o por semilla dentro del bucle de
> `slots`. Es una extensión natural del `match kind` actual.

---

## 6. Cómo se forman aldeas y centros

Contexto para ubicar dónde caen tus construcciones (`src/settlements.rs`):

- **Capital** (`SettlementKind::Capital`): muralla de radio 14, 4 puertas
  cardinales, cruz de caminos y hasta 6 lotes de casas.
- **Aldea** (`SettlementKind::Village`): muralla de radio 9, 4 puertas, cruz de
  caminos y 4 lotes.
- **Muros** de piedra (3 de alto), **puertas** abiertas al nivel del camino,
  **caminos** adoquinados y **caminos regionales** que conectan capital↔aldeas.
- Todo es **determinista** a partir de `WORLD_SEED`: la misma semilla produce el
  mismo trazado y las mismas rotaciones de casa.

El estampado ocurre **después** de terreno, cuevas y vegetación
(`stamp_settlements_in_chunk`), por lo que caminos/muros/casas sobrescriben la
hierba y no vuelve a crecer flora sobre el adoquín.

---

## 7. Buenas prácticas y problemas comunes

- **La casa flota o se hunde**: revisá que la capa `Y=0` sea maciza y que
  `anchor = [w/2, 0, d/2]` (el editor lo pone automáticamente al exportar).
- **La entrada no da al camino**: es normal antes de rotar; el huevo la orienta.
  Asegurate de que el hueco de puerta esté en la pared **sur** (`z = 0`).
- **Se recorta contra la muralla**: reducí el tamaño o el lote; los huevos que
  quedan a menos de 2 celdas del muro se descartan.
- **Materiales raros**: usá solo la paleta soportada; lo desconocido se ignora.
- **Cambios en `house_basic.json` no se ven**: recompilá (está embebido).

---

## 8. Referencia rápida de archivos

| Archivo | Rol |
|---|---|
| `editor/prefab.html` | Editor visual de prefabs. |
| `assets/prefabs/*.json` | Prefabs guardados. |
| `src/prefab.rs` | Carga/parseo, paleta, rotación y colocación. |
| `src/settlements.rs` | Planos de aldea/capital, lotes (huevos) y estampado. |
| `src/realms.rs` | Reinos: dónde hay capitales y aldeas. |
| `src/world.rs` | `enum Material`, `set_voxel`, alturas de terreno. |
