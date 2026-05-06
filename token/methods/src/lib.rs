include!(concat!(env!("OUT_DIR"), "/methods.rs"));

#[cfg(test)]
mod tests {
    #[test]
    fn token_ids_match_generated_method_id() {
        assert_eq!(token_ids::TOKEN_ID, super::TOKEN_ID);
    }
}
