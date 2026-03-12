fn main() {
    slint_build::compile("ui/visualizer.slint").unwrap();
    slint_build::compile("ui/config_panel.slint").unwrap();
}
