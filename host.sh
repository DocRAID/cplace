wasm-pack build --dev client --target web
python3 -m http.server --directory client 4885
