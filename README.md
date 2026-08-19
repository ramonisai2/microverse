# Microverse

Demo en Rust + wgpu: mundo voxel con cámara libre (WASD + mouse).

## Estructura (plan base)

```
microvoxel/
  Cargo.toml          # crate name = microverse
  README.md
  src/
    main.rs           # ventana, loop, input
    camera.rs         # freecam: WASD + mouse look
    world.rs          # voxels + microvoxels 16³
    render.rs         # wgpu mesh + pipeline
    shader.wgsl       # vertex/fragment + lighting
    blur.wgsl         # post-process distancia
```

## Modelo de mundo

- Un **voxel** = 1×1×1 en el mundo.
- Cada voxel puede ocupar una grilla de **microvoxels** (`16³`, bit-packed).
- Material `Dirt` (caras laterales marrón jaspeado; cara superior verde jaspeado).
- El mundo de juego genera columnas de tierra (heightmap); `World::with_dirt_cube()` sigue disponible para un solo cubo.
- **Reinos** (`src/realms.rs`): parches de 30–150 shunks² (16×16) con capital + aldeas;
  el resto es salvaje/neutral. Consulta: `realm_at_shunk` / `realm_info`.
- **Asentamientos** (`src/settlements.rs`): caminos capital↔aldeas, murallas,
  cuatro puertas y lotes `PrefabEgg`. Las casas apuntan a JSON en
  `assets/prefabs/` y se editan desde `editor/prefab.html`.

## Cámara libre

- **WASD** — movimiento en el plano de mirada
- **Space / E** subir, **Ctrl / Q** bajar
- Mouse — rotar (cursor capturado)
- **Escape** — liberar cursor / salir

## Run

```bash
cargo run
```

o `run.bat`.

Partida: `saves/microverse.json` (F5, cada 45 s, y al cerrar).

## Editor de entidades (voxel)

Editor HTML (Three.js) con esqueleto (partes), **32 colores** y **10 paletas**
(`classic`, `pastel`, `earth`, `neon`, `castle`, `ocean`, `sunset`, `forest`, `mono`, `candy`).

1. Abre [`editor/index.html`](editor/index.html) (mejor con `npx serve .` si fallan los módulos).
2. Elige un editor: bípedo, pezuña, cuadrúpedo, arácnido, pez, medusa u objeto.
3. Pon **nombre** al archivo, edita con pincel / borrador / bote / gotero, **Export** → `{nombre}.json`.
4. Copia el export a [`assets/entities/hero.json`](assets/entities/hero.json) y **reinicia** el juego (`run.bat`). Se lee del disco (formato flat o `parts` del editor); no hace falta recompilar.

Formato juego: `{ id, grid, foot_y, palette, voxels: [{x,y,z,c}] }` donde `c` es `0..31`.

Regenerar el héroe procedural por defecto:

```bash
python tools/export_hero.py
```

