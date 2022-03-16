pub struct Conn;

impl Conn {
    pub fn new() -> Self {
        Self
    }
    pub fn handle(&mut self, bytes: &[u8]) {}
    pub fn update(&mut self) {
        dbg!("a");
    }
}
