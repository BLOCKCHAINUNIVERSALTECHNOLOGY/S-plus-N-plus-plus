//! Integration tests — full pipeline: lex → parse → check → run.

use splusnpp::ast::Value;
use splusnpp::vm::execute;

#[test]
fn hello_add() {
    let src = r#"
        fn add(a: Int, b: Int) -> Int { return a + b; }
        fn main() -> Int { return add(20, 22); }
    "#;
    assert_eq!(execute(src).unwrap(), Value::Int(42));
}

#[test]
fn for_break_continue() {
    let src = r#"
        fn main() -> Int {
            let n = 0;
            for i in 0..100 {
                n = n + 1;
                if n == 5 { break; }
            }
            let sum = 0;
            for i in 0..5 {
                if i == 2 { continue; }
                sum = sum + i;
            }
            return n + sum;
        }
    "#;
    // 5 + (0+1+3+4) = 13
    assert_eq!(execute(src).unwrap(), Value::Int(13));
}

#[test]
fn unary_ops() {
    let src = r#"
        fn main() -> Int {
            let x = -42;
            let b = !false;
            if b { return x; }
            return 0;
        }
    "#;
    assert_eq!(execute(src).unwrap(), Value::Int(-42));
}

#[test]
fn list_index_prints() {
    let src = r#"
        import std.collections;
        fn main() {
            let xs = list(10, 20, 30);
            print(xs[1]);
            print(-10);
        }
    "#;
    assert_eq!(execute(src).unwrap(), Value::Unit);
}

#[test]
fn concurrency_roundtrip() {
    let src = r#"
        import std.concurrency;
        fn worker(ch: Channel) {
            send(ch, 99);
        }
        fn main() {
            let ch = channel();
            let t = spawn("worker", ch);
            join(t);
            print(receive(ch));
        }
    "#;
    assert_eq!(execute(src).unwrap(), Value::Unit);
}
