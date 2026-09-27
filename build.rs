fn main() {
    if cfg!(windows) {
        let mut resource = winres::WindowsResource::new();
        resource.set_icon("assets/icon.ico");
        resource.set_manifest_file("assets/app.manifest");
        resource
            .compile()
            .expect("Windows resources should compile");
    }
}
