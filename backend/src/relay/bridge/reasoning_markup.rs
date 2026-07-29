//! Conservative compatibility parsing for reasoning blocks emitted as text.
//
// Structured reasoning fields remain the source of truth. This parser only
// handles a complete block at the beginning of an assistant response.

const MAX_LEADING_WHITESPACE_BYTES: usize = 256;
const MAX_BUFFERED_BYTES: usize = 64 * 1024;
const TAGS: [(&str, &str); 2] = [("<thinking>", "</thinking>"), ("<think>", "</think>")];

fn strip_one_leading_line_break(value: &str) -> &str {
    value
        .strip_prefix("\r\n")
        .or_else(|| value.strip_prefix('\n'))
        .unwrap_or(value)
}

fn is_ignorable_prefix(ch: char) -> bool {
    ch.is_whitespace() || ch == '\u{feff}'
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedReasoningMarkup {
    pub(crate) reasoning: String,
    pub(crate) content: String,
    pub(crate) tag: &'static str,
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn split_leading_whitespace(value: &str) -> Option<usize> {
    let mut end = 0;
    for (index, ch) in value.char_indices() {
        if !is_ignorable_prefix(ch) {
            break;
        }
        end = index + ch.len_utf8();
        if end > MAX_LEADING_WHITESPACE_BYTES {
            return None;
        }
    }
    Some(end)
}

pub(crate) fn split_leading_reasoning_markup(content: &str) -> Option<ParsedReasoningMarkup> {
    let prefix_end = split_leading_whitespace(content)?;
    let rest = &content[prefix_end..];
    let (open, close) = TAGS.iter().find(|(open, _)| {
        rest.len() >= open.len()
            && rest.as_bytes()[..open.len()].eq_ignore_ascii_case(open.as_bytes())
    })?;
    let after_open = &rest[open.len()..];
    let close_pos = find_ascii_case_insensitive(after_open, close)?;
    let after_close = &after_open[close_pos + close.len()..];
    Some(ParsedReasoningMarkup {
        reasoning: after_open[..close_pos].trim().to_string(),
        content: strip_one_leading_line_break(after_close).to_string(),
        tag: open,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Undecided,
    Reasoning,
    Content,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkupChunk {
    pub(crate) reasoning: Option<String>,
    pub(crate) content: Option<String>,
    pub(crate) tag: Option<&'static str>,
    pub(crate) detected: bool,
}

impl MarkupChunk {
    fn empty() -> Self {
        Self {
            reasoning: None,
            content: None,
            tag: None,
            detected: false,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LeadingReasoningMarkupParser {
    state: State,
    buffered: String,
    original_prefix: Option<String>,
    close: Option<&'static str>,
}

impl Default for LeadingReasoningMarkupParser {
    fn default() -> Self {
        Self {
            state: State::Undecided,
            buffered: String::new(),
            original_prefix: None,
            close: None,
        }
    }
}

impl LeadingReasoningMarkupParser {
    pub(crate) fn push(&mut self, fragment: &str) -> MarkupChunk {
        match self.state {
            State::Content => MarkupChunk {
                reasoning: None,
                content: Some(fragment.to_string()),
                tag: None,
                detected: false,
            },
            State::Undecided => {
                self.buffered.push_str(fragment);
                if self.buffered.len() > MAX_BUFFERED_BYTES {
                    self.state = State::Content;
                    return MarkupChunk {
                        reasoning: None,
                        content: Some(std::mem::take(&mut self.buffered)),
                        tag: None,
                        detected: false,
                    };
                }
                let Some(non_ws) = self
                    .buffered
                    .char_indices()
                    .find(|(_, ch)| !is_ignorable_prefix(*ch))
                    .map(|(i, _)| i)
                else {
                    if self.buffered.len() > MAX_LEADING_WHITESPACE_BYTES {
                        self.state = State::Content;
                        return MarkupChunk {
                            reasoning: None,
                            content: Some(std::mem::take(&mut self.buffered)),
                            tag: None,
                            detected: false,
                        };
                    }
                    return MarkupChunk::empty();
                };
                if non_ws > MAX_LEADING_WHITESPACE_BYTES {
                    self.state = State::Content;
                    return MarkupChunk {
                        reasoning: None,
                        content: Some(std::mem::take(&mut self.buffered)),
                        tag: None,
                        detected: false,
                    };
                }
                let rest = &self.buffered[non_ws..];
                let Some((open, close)) = TAGS.iter().find(|(open, _)| {
                    let compared = rest.len().min(open.len());
                    rest.as_bytes()[..compared].eq_ignore_ascii_case(&open.as_bytes()[..compared])
                }) else {
                    self.state = State::Content;
                    return MarkupChunk {
                        reasoning: None,
                        content: Some(std::mem::take(&mut self.buffered)),
                        tag: None,
                        detected: false,
                    };
                };
                if rest.len() < open.len() {
                    return MarkupChunk::empty();
                }
                self.original_prefix = Some(self.buffered[..non_ws + open.len()].to_string());
                self.close = Some(*close);
                self.buffered.drain(..non_ws + open.len());
                self.state = State::Reasoning;
                self.finish_if_ready()
            }
            State::Reasoning => {
                self.buffered.push_str(fragment);
                self.finish_if_ready()
            }
        }
    }

    fn finish_if_ready(&mut self) -> MarkupChunk {
        let Some(close) = self.close else {
            return MarkupChunk::empty();
        };
        let Some(close_pos) = find_ascii_case_insensitive(&self.buffered, close) else {
            if self.buffered.len() > MAX_BUFFERED_BYTES {
                self.state = State::Content;
                return MarkupChunk {
                    reasoning: None,
                    content: Some(format!(
                        "{}{}",
                        self.original_prefix.as_deref().unwrap_or("<thinking>"),
                        std::mem::take(&mut self.buffered)
                    )),
                    tag: None,
                    detected: false,
                };
            }
            return MarkupChunk::empty();
        };
        let reasoning = self.buffered[..close_pos].trim().to_string();
        let content =
            strip_one_leading_line_break(&self.buffered[close_pos + close.len()..]).to_string();
        self.buffered.clear();
        self.state = State::Content;
        MarkupChunk {
            reasoning: (!reasoning.is_empty()).then_some(reasoning),
            content: (!content.is_empty()).then_some(content),
            tag: TAGS
                .iter()
                .find(|(_, candidate_close)| *candidate_close == close)
                .map(|(open, _)| *open),
            detected: true,
        }
    }

    pub(crate) fn finish(&mut self) -> Option<String> {
        let buffered = std::mem::take(&mut self.buffered);
        match self.state {
            State::Reasoning => Some(format!(
                "{}{}",
                self.original_prefix.as_deref().unwrap_or("<thinking>"),
                buffered
            )),
            State::Undecided if !buffered.is_empty() => Some(buffered),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_tags_and_leading_whitespace() {
        assert_eq!(
            split_leading_reasoning_markup(" \n<think>plan</think>\nanswer")
                .unwrap()
                .reasoning,
            "plan"
        );
        assert_eq!(
            split_leading_reasoning_markup("\u{feff}<THINKING>plan</THINKING>\nanswer")
                .unwrap()
                .content,
            "answer"
        );
    }

    #[test]
    fn parser_handles_tag_split_after_newline() {
        let mut parser = LeadingReasoningMarkupParser::default();
        assert_eq!(parser.push("\n  <thi"), MarkupChunk::empty());
        let chunk = parser.push("nking>plan</thinking>\nanswer");
        assert_eq!(chunk.reasoning.as_deref(), Some("plan"));
        assert_eq!(chunk.content.as_deref(), Some("answer"));
        assert_eq!(chunk.tag, Some("<thinking>"));
        assert!(chunk.detected);
    }

    #[test]
    fn unclosed_markup_preserves_original_prefix() {
        let mut parser = LeadingReasoningMarkupParser::default();
        assert!(parser.push(" \n<THINK>unfinished").content.is_none());
        assert_eq!(parser.finish().as_deref(), Some(" \n<THINK>unfinished"));
    }
}
