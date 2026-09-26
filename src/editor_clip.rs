//! Animation clip held by the native editor: import from `assets/animations/`
//! and export to the editor's own folder.
//!
//! Fase 6, paso (C) — the authoring loop: import a clip, refine it, export it.
//! The in-memory representation is [`ClipFile`] (the serializable form), **not**
//! [`crate::animation::AnimationClip`] (which holds radians and cannot be
//! written back). Nothing here changes the game's runtime: a clip is data the
//! editor owns, and `AnimationClip::from_file` turns it into something
//! samplable when the timeline needs to play it.
//!
//! Export deliberately writes to [`clips_dir`], **not** back over the imported
//! file: the future "edited vs imported" diff needs the original to survive as
//! the baseline (see `docs/plan_fase6.md`, decision 3).

use crate::animation::ClipFile;
use std::path::{Path, PathBuf};

/// Where the editor writes clips it has edited. Inside the scene folder, so it
/// travels with the save the way `editor::scene_dir` already does (Android's
/// external dir, desktop `saves/editor`).
pub fn clips_dir() -> PathBuf {
    crate::editor::scene_dir().join("clips")
}

/// A clip open in the editor: the parsed file plus where it came from.
/// `PartialEq` on the clip data only (the paths are provenance, not content).
#[derive(Clone, Debug, PartialEq)]
pub struct EditorClip {
    pub file: ClipFile,
    /// Path it was imported from (`None` if it was built in memory).
    pub source: Option<PathBuf>,
}

impl EditorClip {
    /// Parse a clip from JSON text. Empty `frames` is rejected, same guard the
    /// game's `AnimationClip::from_json_str` uses.
    pub fn from_json(text: &str, source: Option<PathBuf>) -> Result<Self, String> {
        let file: ClipFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if file.frames.is_empty() {
            return Err("clip sin fotogramas".into());
        }
        Ok(Self { file, source })
    }

    /// `assets/animations/*.json` found under the clip search roots, sorted,
    /// deduplicated. Mirrors what the game loads at startup, so anything offered
    /// here is something the game can also read.
    pub fn importable() -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for root in crate::animation::clip_search_paths("assets/animations") {
            let Ok(entries) = std::fs::read_dir(&root) else {
                continue;
            };
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                if !out.contains(&path) {
                    out.push(path);
                }
            }
        }
        out.sort();
        out
    }

    /// Read and parse one of [`Self::importable`]'s paths.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text, Some(path.to_path_buf()))
    }

    /// Write the clip as `<id>.json` into `dir`, overwriting a previous export
    /// but never the source. The dir is a parameter so a test can aim it
    /// somewhere harmless; the editor passes [`clips_dir`].
    pub fn save_to(&self, dir: &Path) -> Result<PathBuf, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(format!("{}.json", crate::editor::safe_name(&self.file.id)));
        let text = serde_json::to_string_pretty(&self.file).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }

    /// `t` of keyframe `i` (clamped to the frames that exist).
    pub fn frames_time(&self, i: usize) -> f32 {
        self.file
            .frames
            .get(i.min(self.file.frames.len().saturating_sub(1)))
            .map(|f| f.t)
            .unwrap_or(0.0)
    }

    /// One-line summary for the panel: `id · N claves · 0.38s · loop`.
    pub fn summary(&self) -> String {        format!(
            "{} · {} claves · {:.2}s · {}",
            self.file.id,
            self.file.frames.len(),
            self.file.duration_s,
            if self.file.looped { "loop" } else { "una vez" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIP: &str = r#"{
        "version": "1.0",
        "kind": "animation",
        "id": "borrador_ia",
        "model": "assets/entities/hero.json",
        "duration_s": 0.4,
        "loop": true,
        "frames": [
            { "t": 0.0, "pose": { "l_arm_x": -20.0 } },
            { "t": 0.4, "pose": { "l_arm_x": 15.0 } }
        ]
    }"#;

    #[test]
    fn a_clip_imports_and_summarises() {
        let c = EditorClip::from_json(CLIP, None).expect("clip");
        assert_eq!(c.file.id, "borrador_ia");
        assert_eq!(c.file.frames.len(), 2);
        assert!(c.file.looped);
        assert_eq!(c.summary(), "borrador_ia · 2 claves · 0.40s · loop");
        assert!(c.source.is_none());
    }

    #[test]
    fn a_clip_without_frames_is_rejected() {
        let bad = r#"{"id": "vacio", "model": "m", "frames": []}"#;
        assert!(EditorClip::from_json(bad, None).is_err());
    }

    /// Exportar no pisa el importado: el diff futuro necesita la línea base.
    #[test]
    fn export_writes_to_the_given_dir_not_over_the_source() {
        let root = std::env::temp_dir().join("microverse_clip_test");
        let _ = std::fs::remove_dir_all(&root);
        // El importado y el exportado en carpetas distintas, como en la vida
        // real (assets/animations/ vs el directorio del editor).
        let imported = root.join("imported");
        let exported = root.join("exported");
        std::fs::create_dir_all(&imported).unwrap();
        let source = imported.join("borrador_ia.json");
        std::fs::write(&source, CLIP).unwrap();

        let mut c = EditorClip::load(&source).expect("load");
        c.file.duration_s = 0.9; // el refinamiento
        let out = c.save_to(&exported).expect("save");

        // El origen conserva el texto original (línea base intacta) y no hay
        // ningún otro fichero escrito a su lado.
        assert_eq!(std::fs::read_to_string(&source).unwrap(), CLIP);
        let mut around: Vec<String> = std::fs::read_dir(&imported)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .collect();
        around.sort();
        assert_eq!(around, vec!["borrador_ia.json".to_string()]);
        // El export está en la otra carpeta, con el cambio, y se reimporta.
        assert!(out.starts_with(&exported));
        let back = EditorClip::load(&out).expect("reimport");
        assert!((back.file.duration_s - 0.9).abs() < 1e-6);
        assert_eq!(back.file.frames.len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Lo que el editor ofrece para importar es lo que el juego puede leer: los
    /// 9 clips incorporados existen en assets/animations/.
    #[test]
    fn importable_lists_the_games_own_clips() {
        let files = EditorClip::importable();
        let names: Vec<String> = files
            .iter()
            .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
            .collect();
        for want in crate::animation::CLIP_NAMES {
            assert!(
                names.iter().any(|n| n == want),
                "falta {want} en {names:?}"
            );
        }
    }
}
