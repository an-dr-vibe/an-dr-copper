fn main() {
    #[cfg(feature = "native-ui")]
    tauri_build::build();
}
