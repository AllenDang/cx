//! Two modules that each define a symbol named `run`.

pub mod alpha {
    /// Returns the alpha tick.
    pub fn run() -> u32 {
        1
    }
}

pub mod beta {
    /// Returns the beta tick.
    pub fn run() -> u32 {
        2
    }
}

pub fn run_both() -> u32 {
    alpha::run() + beta::run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_both_sums_scopes() {
        assert_eq!(run_both(), 3);
    }
}
