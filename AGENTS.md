# AGENTS.md — reglas de trabajo para agentes

Reglas que el agente (opencode, aider, cursor…) debe aplicar **siempre**, en cada
entrega, sin que nadie se lo recuerde. El resto de convenciones del repo están en
`CONVENTIONS.md`, que también hay que leer.

## 1. Commit de un solo fichero: compilar el commit aislado

No basta con leer el diff. Un commit de un solo fichero se verifica compilando
**ese commit**, en un worktree aparte. El método que funcionó en `c78af21`:

```bash
WT=/tmp/opencode/microvoxel-verify
git worktree remove --force "$WT" 2>/dev/null || true
rm -rf "$WT"
git worktree add --detach "$WT" HEAD
CARGO_TARGET_DIR="$PWD/target-linux" cargo check --manifest-path "$WT/Cargo.toml"
git worktree remove --force "$WT"
```

Detalles que importan:

- `CARGO_TARGET_DIR` apunta al `target-linux` del repo principal: reutiliza las
  dependencias ya compiladas y no toca `target/` (basura de Windows). Es un
  `cargo check`, no un binario viejo.
- Verificar **después** de comitear, sobre `HEAD`, no sobre el working tree: si
  compila el árbol de trabajo puede no ser el commit lo que se probó.
- Si el commit no compila, se corrige con un commit nuevo. **No reescribir
  historia** (nada de `rebase`, `amend` ni `reset` sobre commits ya entregados).

## 2. Revisar también mi propio texto

El diff no es lo único que se entrega. Antes de mandar cualquier respuesta o
cambio:

1. Leer la propia respuesta completa de arriba abajo, buscando basura pegada
   dentro de una palabra.
2. Pasar el checker a lo que se entrega o comitea:

```bash
python3 tools/check_text.py $(git diff --name-only) CONVENTIONS.md AGENTS.md
```

Sale 1 si algo salta, 0 si está limpio.

El checker ve **caracteres**, no empalmes: una corrupción dentro de ASCII
(`losampoco` por "lo tampoco") pasa el test. Por eso la lectura manual no es
opcional, y por eso hay que hacerlo aunque el diff esté impecable.

## Lo que NO se toca

- No reescribir historial.
- No tocar `saves/`, `target/`, `target-linux/`.
- No dar por buena una entrega sin las dos verificaciones de arriba.
