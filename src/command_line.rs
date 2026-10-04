//! Parse a terminal task command line into a [`ToolCommand`], without a shell.
//!
//! Splitting is done here so the parsed program and arguments can be spawned
//! directly: nothing is ever executed during parsing, and `$VAR`, `$(...)`,
//! backticks and glob characters are kept as literal text, never expanded.

use anyhow::{Result, bail};

use crate::config::ToolCommand;

/// Largest accepted command line, in bytes.
const MAX_INPUT_BYTES: usize = 8192;

/// Parse `input` into a program plus literal arguments.
///
/// Arguments are split on Unicode whitespace outside quotes. Single quotes
/// preserve everything; double quotes preserve whitespace and let a backslash
/// escape the next character; outside quotes a backslash also escapes the
/// next character. Adjacent quoted and unquoted segments form one argument,
/// and quoted empty arguments are kept. Unquoted `| & ; < >` are rejected:
/// shell control flow requires an explicit shell invocation.
pub fn parse(input: &str) -> Result<ToolCommand> {
    if input.len() > MAX_INPUT_BYTES {
        bail!(
            "command line is {} bytes; the limit is {MAX_INPUT_BYTES}",
            input.len()
        );
    }
    for c in input.chars() {
        // Tabs are ordinary separating whitespace; anything else
        // non-printable (NUL, newline, carriage return, ...) is rejected.
        if c != '\t' && c.is_control() {
            bail!(
                "command contains control character {:?}; only printable text and tabs are allowed",
                c.escape_debug()
            );
        }
    }

    let mut args = split_words(input)?.into_iter();
    let Some(program) = args.next() else {
        bail!("empty command: enter a program and optional arguments");
    };
    if program.is_empty() {
        bail!("program name is empty; start the command with a program name");
    }
    Ok(ToolCommand {
        program,
        args: args.collect(),
    })
}

/// Whitespace-split `input` following the quoting and escaping rules above.
fn split_words(input: &str) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    // Tracks whether an argument has started, so `""` survives as an empty
    // argument while runs of whitespace between arguments collapse.
    let mut started = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            if started {
                args.push(std::mem::take(&mut current));
                started = false;
            }
            continue;
        }
        match c {
            '|' | '&' | ';' | '<' | '>' => bail!(
                "unquoted `{c}` is a shell operator and is not interpreted; \
                 run a shell script explicitly instead, e.g. program `sh` \
                 with args `-c \"a {c} b\"`"
            ),
            '\\' => match chars.next() {
                Some(escaped) => {
                    started = true;
                    current.push(escaped);
                }
                None => bail!("command ends with an unpaired backslash"),
            },
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        None => bail!("unmatched single quote: add a closing `'`"),
                        Some('\'') => break,
                        Some(c) => current.push(c),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        None => bail!("unmatched double quote: add a closing `\"`"),
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(escaped) => current.push(escaped),
                            None => bail!("unmatched double quote: add a closing `\"`"),
                        },
                        Some(c) => current.push(c),
                    }
                }
            }
            // Outside of a quoted run the segment has begun even for plain
            // characters; `started` may already be set by a joined segment.
            c => {
                started = true;
                current.push(c);
            }
        }
    }
    if started {
        args.push(current);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &str) -> (String, Vec<String>) {
        let command = parse(input).expect("should parse");
        (command.program, command.args)
    }

    #[test]
    fn parses_a_plain_command() {
        assert_eq!(
            parsed("cargo test --lib"),
            ("cargo".into(), vec!["test".into(), "--lib".into()])
        );
    }

    #[test]
    fn tabs_and_unicode_whitespace_separate() {
        // U+00A0 (no-break space) and U+3000 (ideographic space) separate.
        assert_eq!(
            parsed("echo\u{a0}ok\u{3000}done"),
            ("echo".into(), vec!["ok".into(), "done".into()])
        );
    }

    #[test]
    fn quoted_whitespace_stays_inside_one_argument() {
        assert_eq!(
            parsed(r#"bash -c "echo hello world""#),
            ("bash".into(), vec!["-c".into(), "echo hello world".into()])
        );
    }

    #[test]
    fn the_program_itself_can_be_quoted() {
        assert_eq!(
            parsed(r#""my program" run"#),
            ("my program".into(), vec!["run".into()])
        );
    }

    #[test]
    fn empty_quoted_arguments_are_preserved() {
        assert_eq!(
            parsed(r#"run "" x ''"#),
            ("run".into(), vec!["".into(), "x".into(), "".into()])
        );
    }

    #[test]
    fn adjacent_segments_form_one_argument() {
        assert_eq!(
            parsed(r#"pre"mid dle"post'x'"#),
            ("premid dlepostx".into(), vec![])
        );
        assert_eq!(parsed(r#""a b"c d"#), ("a bc".into(), vec!["d".into()]));
    }

    #[test]
    fn single_quotes_preserve_everything() {
        assert_eq!(
            parsed(r"echo 'C:\path\next'"),
            ("echo".into(), vec![r"C:\path\next".into()])
        );
    }

    #[test]
    fn backslash_escapes_the_next_character() {
        assert_eq!(
            parsed(r#"a\"b "c\"d" e\\f g\ h"#),
            (
                "a\"b".into(),
                vec!["c\"d".into(), "e\\f".into(), "g h".into()]
            )
        );
    }

    #[test]
    fn dollar_backtick_and_glob_forms_are_literal() {
        assert_eq!(
            parsed("echo '$HOME' $(date) `id` *"),
            (
                "echo".into(),
                vec!["$HOME".into(), "$(date)".into(), "`id`".into(), "*".into()]
            )
        );
    }

    #[test]
    fn unicode_content_is_preserved() {
        assert_eq!(
            parsed("echo 日本語 \"Ün ix\" ﷼"),
            (
                "echo".into(),
                vec!["日本語".into(), "Ün ix".into(), "﷼".into()]
            )
        );
    }

    #[test]
    fn unquoted_shell_operators_are_rejected() {
        for op in ['|', '&', ';', '<', '>'] {
            let err = parse(&format!("echo a {op} b")).expect_err("operator should be rejected");
            let msg = err.to_string();
            assert!(
                msg.contains("shell") && msg.contains(&format!("`{op}`")),
                "message for {op:?} should be actionable: {msg}"
            );
        }
    }

    #[test]
    fn quoted_or_escaped_operators_are_literal() {
        assert_eq!(
            parsed(r#"sh -c "echo a | b ; c" 'd & e' \< \> \; \&"#),
            (
                "sh".into(),
                vec![
                    "-c".into(),
                    "echo a | b ; c".into(),
                    "d & e".into(),
                    "<".into(),
                    ">".into(),
                    ";".into(),
                    "&".into(),
                ]
            )
        );
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in [
            "",
            "   ",
            "\t",
            "echo 'unterminated",
            "echo \"unterminated",
            "echo \"", // escaped closing quote leaves it unmatched
            "echo \\",
            "echo a\nb",
            "echo a\r\nb",
            "echo a\u{0}b",
            "echo a\u{1}b",
            "echo \\\u{a}",
        ] {
            assert!(parse(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn error_messages_point_at_the_problem() {
        assert!(parse("").unwrap_err().to_string().contains("empty"));
        assert!(
            parse(r#""" x"#)
                .unwrap_err()
                .to_string()
                .contains("program")
        );
        assert!(
            parse("echo \\")
                .unwrap_err()
                .to_string()
                .contains("backslash")
        );
        assert!(
            parse("echo 'x")
                .unwrap_err()
                .to_string()
                .contains("single quote")
        );
    }

    #[test]
    fn enforces_the_input_size_limit() {
        assert!(parse(&"a".repeat(MAX_INPUT_BYTES)).is_ok());
        assert!(parse(&"a".repeat(MAX_INPUT_BYTES + 1)).is_err());
    }
}

#[test]
fn escaped_only_argument() {
    assert_eq!(parse(r"echo \;").unwrap().args, vec![";"]);
}
