use crate::scanner::{Scanner, Token, TokenType};

pub(crate) fn compile(source: String) {
    let chars: Vec<char> = source.chars().collect();
    let mut scanner: Scanner = Scanner::init(0, source.len(), &chars);
    let mut line: i32 = -1;

    loop {
        let token: Token = scanner.scan_token();

        if token.line != line {
            print!("{:?}: ", token.line);
            line = token.line;
        } else {
            print!("    | ");
        }

        println!("{:?}", token);

        if token.token_type == TokenType::Eof {
            break;
        }
    }
}
