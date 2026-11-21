// Bettet apps.json und die Programmsymbole (icons/*.png) aus dem Hauptordner
// des Repos ein. Dort müssen sie bleiben: ältere Launcher lesen sie direkt
// von GitHub (main/apps.json, main/icons/…).
use std::{env, fs, path::Path};

fn main() {
    let wurzel = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut code = String::from("pub static SYMBOLE: &[(&str, &[u8])] = &[\n");
    let icons = wurzel.join("icons");
    if let Ok(liste) = fs::read_dir(&icons) {
        let mut namen: Vec<_> = liste.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "png").unwrap_or(false)).collect();
        namen.sort();
        for p in namen {
            let name = format!("icons/{}", p.file_name().unwrap().to_string_lossy());
            code.push_str(&format!("    ({name:?}, include_bytes!({:?})),\n", p.canonicalize().unwrap()));
        }
    }
    code.push_str("];\n");
    code.push_str(&format!("pub static APPS_JSON: &str = include_str!({:?});\n", wurzel.join("apps.json").canonicalize().unwrap()));
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("eingebettet.rs"), code).unwrap();
    println!("cargo:rerun-if-changed=../apps.json");
    println!("cargo:rerun-if-changed=../icons");
    tauri_build::build()
}
