fn main() {
    let a: Vec<String> = std::env::args().collect();
    let core = std::fs::read(&a[1]).expect("read core module");
    let c = wit_component::ComponentEncoder::default().module(&core).expect("module").validate(true).encode().expect("encode");
    std::fs::write(&a[2], c).expect("write");
}
