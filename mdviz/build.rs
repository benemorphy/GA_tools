fn main() {
    println!("cargo::rerun-if-changed=mdviz.rc");
    println!("cargo::rerun-if-changed=mdviz_icon.ico");
    let _ = embed_resource::compile("mdviz.rc", [] as [&str; 0]);
}
