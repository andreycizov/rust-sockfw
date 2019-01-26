#[cfg(test)]
mod tests;

struct RingBuffer {
    b: Vec<u8>,
    up: usize,
    down: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        RingBuffer {
            b: vec![0; capacity],
            up: 0,
            down: 0
        }
    }
}