fn main() {
    println!("cargo:rerun-if-changed=res/res.rc");
    println!("cargo:rerun-if-changed=res/res.h");
    for icon in ["azookey.ico", "kana_black.ico", "latin_black.ico", "kana_white.ico", "latin_white.ico"] {
        println!("cargo:rerun-if-changed=res/{icon}");
    }
    let _ = embed_resource::compile("res/res.rc", embed_resource::NONE);
}
