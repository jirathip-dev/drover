pub fn f(x: &[u8], p: &std::path::Path) -> Option<u8> {
    std::fs::remove_file(p).ok();
    u8::try_from(u32::from(*x.first()?)).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        assert_eq!(super::f(&[1], std::path::Path::new("/nonexistent")).unwrap(), [1][0]);
    }
}
