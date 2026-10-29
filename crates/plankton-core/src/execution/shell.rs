//! A deliberately small shell recognizer, not a shell interpreter. Captured shell
//! text remains the execution input; this only finds statically identifiable consumers.
#[derive(Debug, Clone)]
pub(super) struct Word {
    pub text: String,
    pub dynamic: bool,
    quoted: bool,
    operator: bool,
    here_document: Option<String>,
}

#[derive(Debug)]
pub(super) struct Command {
    pub words: Vec<Word>,
    pub stdin: Option<String>,
    pub stdin_path: Option<Word>,
    pub output_paths: Vec<Word>,
}

pub(super) fn commands(source: &str) -> Result<Vec<Command>, String> {
    let tokens = tokenize(source)?;
    let mut commands = Vec::new();
    let mut current = Command {
        words: Vec::new(),
        stdin: None,
        stdin_path: None,
        output_paths: Vec::new(),
    };
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        if token.operator {
            let operator = token
                .text
                .trim_start_matches(|ch: char| ch.is_ascii_digit());
            let descriptor = &token.text[..token.text.len() - operator.len()];
            match operator {
                ";" | "|" | "&&" | "||" => {
                    if !current.words.is_empty() {
                        commands.push(current);
                    }
                    current = Command {
                        words: Vec::new(),
                        stdin: None,
                        stdin_path: None,
                        output_paths: Vec::new(),
                    };
                }
                ">" | ">>" | "<" | "<<" => {
                    let target = tokens
                        .get(index + 1)
                        .filter(|word| !word.operator)
                        .ok_or("shell redirection requires a literal target")?;
                    if (operator == "<" || operator == "<<")
                        && !descriptor.is_empty()
                        && descriptor != "0"
                    {
                        return Err(
                            "only file descriptor 0 input can be captured as consumer stdin".into(),
                        );
                    }
                    if operator == "<" {
                        current.stdin_path = Some(target.clone());
                        current.stdin = None;
                    }
                    if operator == "<<" {
                        current.stdin = Some(
                            target
                                .here_document
                                .clone()
                                .ok_or("heredoc body is missing")?,
                        );
                        current.stdin_path = None;
                    }
                    if operator == ">" || operator == ">>" {
                        current.output_paths.push(target.clone());
                    }
                    index += 1;
                }
                _ => {
                    return Err(format!(
                        "shell syntax {} has no execution-evidence recognizer yet",
                        token.text
                    ))
                }
            }
        } else {
            current.words.push(token.clone());
        }
        index += 1;
    }
    if !current.words.is_empty() {
        commands.push(current);
    }
    Ok(commands)
}

fn tokenize(source: &str) -> Result<Vec<Word>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::<Word>::new();
    let mut word = String::new();
    let mut dynamic = false;
    let mut quoted = false;
    let mut quote = None;
    let mut active = false;
    let mut pending_here = Vec::<usize>::new();
    let mut index = 0;
    let flush = |tokens: &mut Vec<Word>,
                 word: &mut String,
                 dynamic: &mut bool,
                 quoted: &mut bool,
                 active: &mut bool,
                 pending: &mut Vec<usize>| {
        if !*active {
            return;
        }
        let is_delimiter = tokens
            .last()
            .is_some_and(|token| token.operator && token.text.ends_with("<<"));
        tokens.push(Word {
            text: std::mem::take(word),
            dynamic: *dynamic,
            quoted: *quoted,
            operator: false,
            here_document: None,
        });
        if is_delimiter {
            pending.push(tokens.len() - 1);
        }
        *dynamic = false;
        *quoted = false;
        *active = false;
    };
    while index < chars.len() {
        let ch = chars[index];
        if quote == Some('\'') {
            if ch == '\'' {
                quote = None;
            } else {
                word.push(ch);
            }
            index += 1;
            continue;
        }
        if ch == '\\' {
            let next = *chars.get(index + 1).ok_or("unfinished shell escape")?;
            if quote == Some('"') && !['$', '`', '"', '\\', '\n'].contains(&next) {
                word.push('\\');
                active = true;
                index += 1;
                continue;
            }
            if next != '\n' {
                word.push(next);
                active = true;
            }
            index += 2;
            continue;
        }
        if ch == '`'
            || (ch == '$'
                && chars
                    .get(index + 1)
                    .is_some_and(|next| ['(', '[', '\'', '"'].contains(next)))
        {
            return Err("shell command substitution cannot be bound yet; move it into a readable consumer script".into());
        }
        if ch == '$' && chars.get(index + 1) == Some(&'{') {
            let tail = &chars[index + 2..];
            let end = tail
                .iter()
                .position(|ch| *ch == '}')
                .ok_or("unfinished shell parameter reference")?;
            let name = &tail[..end];
            if name.is_empty()
                || !name
                    .iter()
                    .all(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            {
                return Err("only simple shell variable references can be bound; parameter operators are unsupported".into());
            }
        }
        if quote == Some('"') {
            if ch == '"' {
                quote = None;
            } else {
                word.push(ch);
                dynamic |= ch == '$';
            }
            index += 1;
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            quoted = true;
            active = true;
            index += 1;
            continue;
        }
        if ch == '#' && !active {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if ch.is_whitespace() {
            flush(
                &mut tokens,
                &mut word,
                &mut dynamic,
                &mut quoted,
                &mut active,
                &mut pending_here,
            );
            index += 1;
            if ch == '\n' {
                for target_index in std::mem::take(&mut pending_here) {
                    let target = &mut tokens[target_index];
                    if target.dynamic || target.text.is_empty() {
                        return Err("heredoc delimiter must be literal".into());
                    }
                    let mut content = String::new();
                    loop {
                        if index >= chars.len() {
                            return Err("heredoc delimiter was not closed".into());
                        }
                        let start = index;
                        while index < chars.len() && chars[index] != '\n' {
                            index += 1;
                        }
                        let line: String = chars[start..index].iter().collect();
                        let newline = index < chars.len();
                        if newline {
                            index += 1;
                        }
                        if line == target.text {
                            break;
                        }
                        content.push_str(&line);
                        if newline {
                            content.push('\n');
                        }
                    }
                    if !target.quoted
                        && (content.contains('$')
                            || content.contains('`')
                            || content.contains('\\'))
                    {
                        return Err(
                            "expanding heredoc source cannot be bound; quote its delimiter".into(),
                        );
                    }
                    target.here_document = Some(content);
                }
                tokens.push(Word {
                    text: ";".into(),
                    dynamic: false,
                    quoted: false,
                    operator: true,
                    here_document: None,
                });
            }
            continue;
        }
        if "|&;<>()".contains(ch) || ch == '>' {
            // A descriptor prefix belongs to the redirection, not the child argv.
            let mut descriptor = String::new();
            if (ch == '>' || ch == '<')
                && active
                && !quoted
                && word.chars().all(|value| value.is_ascii_digit())
            {
                descriptor = std::mem::take(&mut word);
                active = false;
                dynamic = false;
            }
            flush(
                &mut tokens,
                &mut word,
                &mut dynamic,
                &mut quoted,
                &mut active,
                &mut pending_here,
            );
            let mut operator = descriptor;
            operator.push(ch);
            if chars.get(index + 1) == Some(&ch) && "|&<>".contains(ch) {
                operator.push(ch);
                index += 1;
            }
            tokens.push(Word {
                text: operator,
                dynamic: false,
                quoted: false,
                operator: true,
                here_document: None,
            });
            index += 1;
            continue;
        }
        dynamic |= ch == '$' || "*?[]".contains(ch) || (ch == '~' && !active);
        active = true;
        word.push(ch);
        index += 1;
    }
    if quote.is_some() {
        return Err("unfinished shell quotation".into());
    }
    flush(
        &mut tokens,
        &mut word,
        &mut dynamic,
        &mut quoted,
        &mut active,
        &mut pending_here,
    );
    if !pending_here.is_empty() {
        return Err("heredoc body is missing".into());
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_quoted_inline_and_heredoc_sources_across_pipelines() {
        let parsed = commands(
            "python3 - <<'PY' | curl --data-binary @- https://example.test\nprint('a|b')\nPY\n",
        )
        .unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].stdin.as_deref(), Some("print('a|b')\n"));
        assert_eq!(parsed[1].words[0].text, "curl");
        let parsed = commands("python3 -c 'print(\"a|b\")' && node -e 'console.log(1)' ").unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].words[2].text, "print(\"a|b\")");
        assert!(commands("curl $(cat target)").is_err());
        assert!(commands("python3 - <<PY\nprint('$TOKEN')\nPY\n").is_err());
    }

    #[test]
    fn double_quote_escapes_match_shell_path_semantics() {
        let parsed = commands(r#"python3 "a\b.py" "escaped\$literal" "back\\slash""#).unwrap();
        assert_eq!(parsed[0].words[1].text, r"a\b.py");
        assert_eq!(parsed[0].words[2].text, "escaped$literal");
        assert!(!parsed[0].words[2].dynamic);
        assert_eq!(parsed[0].words[3].text, r"back\slash");
    }
}
