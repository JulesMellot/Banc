fn main() {
    // `include_dir!` ne signale pas à cargo les fichiers ajoutés au dossier.
    println!("cargo:rerun-if-changed=../../migrations");
}
