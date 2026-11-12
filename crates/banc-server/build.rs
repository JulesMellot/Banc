fn main() {
    // `include_dir!` does not tell cargo about files added to the folder.
    println!("cargo:rerun-if-changed=../../migrations");
}
