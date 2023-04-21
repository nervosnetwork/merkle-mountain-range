fn main() {
    println!("cargo:rerun-if-changed=../ckb_mmr.h");
    println!("cargo:rerun-if-changed=./mmr.c");

    cc::Build::new()
        .file("./mmr.c")
        .flag("-O3")
        .flag("-Wall")
        .flag("-Werror")
        .flag("-DBLAKE2_REF_C")
        .include(".")
        .include("..")
        .compile("mmr_c");
}
