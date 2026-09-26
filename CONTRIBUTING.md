# Kontribusi

1. Fork, buat branch dari `main`.
2. Sebelum commit: `cargo fmt && cargo clippy -- -D warnings && cargo build --release`.
3. Tes minimal dengan dua komputer (atau satu komputer + VM) dan sebutkan distro serta desktop yang dites di PR.
4. Selalu siapkan cara keluar saat testing: `Ctrl+Alt+Shift+Esc`, atau jalankan dari SSH supaya bisa di-kill kalau input nyangkut.
