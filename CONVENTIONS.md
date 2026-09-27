# Microverse — instrucciones para Aider

El usuario habla español. Responde en español. Crate: `microverse`. Juego voxel **HD-2D** (no FPS). Rust + wgpu.

Lee este archivo entero antes de editar. Prefiere cambios pequeños. No reescribas `render.rs` ni `world.rs` enteros.

## No inventes datos (anti-alucinación)

Esto manda sobre cualquier intuición, tutorial de Minecraft/wgpu, o conversación anterior.

- **Solo afirma lo que esté en el repo.** Constantes, flags, nombres de funciones, teclas, rutas, tamaños de mesh, distancias, semillas: léelos del archivo actual. Si no lo abriste en este turno, no recites el número.
- **No rellenes huecos.** Si no está en el código, no lo inventes (“seguro que hay un `FOG_START`”, “F6 guarda”, APIs de wgpu/winit que no usamos, materiales, biomas, items). Di “no lo veo en el código” y pregunta o busca con grep.
- **Cita la fuente.** Al explicar un valor, nombra archivo + símbolo (`HD2D_DIRT_MESH_DIST` en `src/world.rs`). No parafrasees un número “de memoria”.
- **Este `CONVENTIONS.md` puede estar desactualizado.** Si el código contradice este archivo, gana el código. No “corrijas” el juego para que coincida con un resumen viejo.
- **No simules haber corrido el juego** ni inventes FPS, logs, capturas o resultados de `cargo`. Si no ejecutaste el comando, no des la salida.
- **No copies de otros engines.** Microverse no es Minecraft. No portes sistemas (chunk 16×256, lightmap, redstone, etc.) salvo que el usuario lo pida y el código actual lo permita.
- **Diff mínimo y verdadero.** No añadas archivos, constantes o pases “por si acaso”. Si una función ya existe, úsala; no dupliques con otro nombre.
- **Incertidumbre explícita.** Ante duda: (1) leer el archivo, (2) si sigue ambiguo, preguntar al usuario. Nunca adivinar un diseño y presentarlo como hecho.

## Texto corrupto: pasa el checker antes de entregar

Un texto generado puede traer basura pegada dentro de una palabra: Hangul, CJK, un fragmento en inglés, una ligadura. Ha pasado cuatro veces en un solo turno, una de ellas con la herramienta ya disponible. No es un problema de UTF-8 — el archivo se guarda con esos bytes dentro y el diff los enseña como si fueran texto.

```bash
python3 tools/check_text.py $(git diff --name-only) CONVENTIONS.md
```

Sale 1 si algo salta, 0 si está limpio. Pásalo por **todo** lo que entregues o comitees, y también por tu propia respuesta antes de mandarla.

Por qué no es "¿es ASCII?": el repo está en español con rayas, flechas, `+-`, `<=`, así que el test va al revés — un carácter es sospechoso **solo si no aparece ya en algún archivo del repo** (el repo es el corpus de lo que es tipografía legítima aquí). Encima hay una lista dura de scripts que este proyecto nunca usa (Hangul, CJK, cirílico, hebreo, árabe, devanagari, thai, formas de ancho completo), para que una corrupción que llegue a comitearse no se vuelva "legítima" por estar en el corpus. También marca caracteres de control y U+FFFD.

**Límite conocido:** ve caracteres, no empalmes. Una corrupción dentro de ASCII (`seinna` por "se ancla") pasa el checker. Es necesaria, no suficiente: lee el diff igual. Apúntala a lo que vas a entregar, no al repo entero — las transcripciones `session-*.md` citan corrupciones a propósito.

## Cómo correr (Linux)

Juego:

```bash
cd "/run/media/ramon-isai-campos-soto/223 GB/microvoxel"
./run.sh
```

Aider (Qwen3.8 27B):

```bash
"/run/media/ramon-isai-campos-soto/223 GB/microvoxel.sh"
```

`run.sh` hace `cargo run --release` con `CARGO_TARGET_DIR=target-linux`. No ejecutes un binario viejo a mano: hay que recompilar. Windows dejó basura en `target/`; no uses ese directorio en Linux.

GPU: NVIDIA RTX 2060 SUPER, **8 GB VRAM**. Elige el adaptador discreto. Variable `MICROVERSE_GPU` (índice o substring del nombre). No asumas 24 GB de VRAM.

## Qué es el juego

- Mundo voxel infinito, semilla fija `WORLD_SEED` (`src/world.rs`).
- Cámara HD-2D: yaw 45°, pitch −35°, FOV **25°**, distancia ~16. Archivo: `src/camera.rs`.
- Spawn pradera: `(8, 24)` en `src/biomes.rs`.
- Jugador: andar / sprint / salto, 4 corazones, pico, inventario 9×3, excavación en escalera.
- Biomas (`biomes.rs`) ≠ reinos políticos (`realms.rs`) + asentamientos/prefabs (`settlements.rs`, `prefab.rs`).
- Cuevas: `terrain_height` vs `column_height`. Shunks 16×16, altura mundo 64 (`CHUNK_SECTION_HEIGHT` 32 × 2).
- Un voxel = 1×1×1. Microvoxels 16³ bit-packed dentro de un voxel.

## Mapa de archivos

| Archivo | Rol |
|---|---|
| `src/main.rs` | Loop, input, ticks, HUD, save on exit |
| `src/player.rs` | Física, AABB, corazones |
| `src/camera.rs` | HD-2D + frustum |
| `src/world.rs` | Voxels, gen, streaming de shunks |
| `src/render.rs` | wgpu, meshes, screen-door, GPU pick |
| `src/save.rs` | `saves/microverse.json` |
| `src/mining.rs` / `stair_dig.rs` | Picar / túnel escalera |
| `src/hero.rs` `hero_pose.rs` `animation.rs` | Modelo / pose del héroe |
| `src/inventory.rs` `items.rs` `hud.rs` | Inventario y UI |
| `src/shader.wgsl` | Vertex/fragment |

Partida: F5, autosave 45 s, al cerrar. Carga al arrancar. Solo se guardan pose, inventario, durabilidad del pico y **edits sparse** (`set_voxel_player` / `remove_voxel_player`). El terreno procedural se regenera de la semilla.

## Regla dura: unificar oclusión del héroe

Si el héroe queda tapado (hojas, tronco, tierra, techo de cueva), es **un solo problema**. Un pass / una regla / un flag.

Ya existe: `ENABLE_PLAYER_SCREEN_DOOR` + `ENABLE_PLAYER_OCCLUDED_BAYER` en `world.rs` (mismo Bayer bajo árbol y bajo tierra). `ENABLE_DIG_CUTAWAY` es el agujero circular en la tapa sin cavar; no dupliques ese look con otro sistema.

**Prohibido** añadir “solo para cuevas” o “solo para árboles”: segundo Bayer, disco de pies, burial fade, pass extra, curva distinta. Pregunta primero si debe verse igual. Separar solo si el usuario pide looks distintos o si unificar rompe un caso de forma demostrable.

## Streaming y huecos 16×16 (no “arreglarlo” otra vez mal)

- `VIEW_DISTANCE` = 96 es burbuja de streaming (legado FPS). El corte de mesh HD-2D es `HD2D_DIRT_MESH_DIST` (72) desde el **focus del jugador**, no desde la cámara.
- Hierba se acorta en `GRASS_BLADE_FADE_*` (68→73). Niebla `FOG_DENSITY` 0.007 (~40% al corte). La cámara está ~16 por el rayo de mirada. No subas el disco otra vez a 96.
- El anillo keep (`fill_keep_ring`) se genera en oleadas y se comitea con **tope por tick** (`INNER_COMMIT_CAP` = 4 dentro de la corona visible): comitear la corona entera de golpe son ~250 ms de hitch cuando el origen cruza un chunk. El frente visible nunca se para (el chunk bajo el héroe va primero y se rellena en el primer tick) y la corona converge en los siguientes ticks — en el arranque/teleporte eso lo tapa la pantalla de carga. No subir el cap "para que sea todo de golpe": es el hitch. No dejar shunks `Reserved` vacíos a la vista. Invariante fijada por `stream_seals_visible_crown_over_ticks` y `fill_keep_ring_never_starves_the_visible_crown`.
- Radio de preload = anillo keep (no 4 shunks).
- Meshear chunks sin mesh GPU aunque el budget de tiempo esté justo; no tirar el mesh a medias.
- FOV 25°: **no frustum-cull** shunks que siguen en pantalla (ya se rompió así).
- No meter la facing de la cámara en `chunk_state_key`.
- No cull por cámara las caras greedy de tierra de forma que dejen paredes-rejilla / vacío en vecinos.

Si ves huecos: confirma que corriste `./run.sh` (rebuild). Un binario stale miente.

## Estilo de código

- Constantes de mundo/render en `world.rs` (flags `ENABLE_*`, distancias HD-2D). No copies números mágicos en tres sitios.
- Determinismo: gen a partir de `WORLD_SEED` + `mix_seed`. No uses `rand` en gen de terreno.
- No toques `saves/`, `target/`, `target-linux/`.
- No añadas dependencias Cargo sin preguntar.
- No reescribas README salvo que el usuario lo pida. Este archivo manda sobre el README (el README aún habla de cámara libre FPS; eso está desactualizado).

## Hardware / Aider local

Ollama: Aider usa `qwen3.8:27b` (~18 GB Q4 en disco) desde `microvoxel.sh`. **No cabe en 8 GB VRAM** — offload a RAM, muy lento. El script hace `ollama pull` si falta (hace falta Ollama ≥ ~0.32.12).
