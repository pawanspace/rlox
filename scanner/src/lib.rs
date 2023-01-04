#[macro_use]
mod scanner;
mod token;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::Scanner;
    use crate::token::TokenType;

    #[test]
    fn scan_tokens_for_valid_input() {
        let input = "var name=\"pawan\"".to_string();
        let mut scanner = Scanner::init(0, input.len(), input.chars().collect());
        let tokens = scanner.scan();
        assert_eq!(tokens.len(), 5);
        println!("{:?}", tokens);
        let result = tokens.iter().find(|token| token.token_type == TokenType::Error);
        assert!(matches!(result, None));
    }

    #[test]
    fn scan_tokens_for_invalid_input() {
        let input = "name==\"pawan\"\"".to_string();
        let mut scanner = Scanner::init(0, input.len(), input.chars().collect());
        let tokens = scanner.scan();
        println!("{:?}", tokens);
        let result = tokens.iter().find(|token| token.token_type == TokenType::Error);
        assert!(matches!(result, Some(_)));
    }
}
