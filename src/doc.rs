//! A small pretty-printing algebra (Wadler style): groups that stay flat when they fit and break otherwise.

#[derive(Clone, Debug)]
pub enum Doc {
    Text(String),
    /// A space when flat, a newline when the enclosing group is broken.
    Line,
    /// Nothing when flat, a newline when broken.
    Soft,
    /// Always a newline; forces every enclosing group to break.
    Hard,
    Group(Vec<Doc>),
    Indent(Vec<Doc>),
}

pub fn text(s: impl Into<String>) -> Doc {
    Doc::Text(s.into())
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Flat,
    Break,
}

fn width(s: &str) -> usize {
    s.chars().count()
}

/// Whether `docs` (in `mode`) fit into `rem` columns up to the next newline.
fn fits<'a>(
    mut rem: isize,
    items: &mut Vec<(usize, Mode, &'a Doc)>,
    rest: &[(usize, Mode, &'a Doc)],
) -> bool {
    let mut rest_idx = rest.len();
    loop {
        let (ind, mode, d) = match items.pop() {
            Some(x) => x,
            None => {
                if rest_idx == 0 {
                    return true;
                }
                rest_idx -= 1;
                rest[rest_idx]
            }
        };
        if rem < 0 {
            return false;
        }
        match d {
            Doc::Text(s) => {
                if s.contains('\n') {
                    return true;
                }
                rem -= width(s) as isize;
            }
            Doc::Line => {
                if mode == Mode::Flat {
                    rem -= 1;
                } else {
                    return true;
                }
            }
            Doc::Soft => {
                if mode == Mode::Break {
                    return true;
                }
            }
            Doc::Hard => return mode == Mode::Break,
            Doc::Group(v) => {
                let m = if mode == Mode::Flat {
                    Mode::Flat
                } else {
                    Mode::Break
                };
                for c in v.iter().rev() {
                    items.push((ind, m, c));
                }
            }
            Doc::Indent(v) => {
                for c in v.iter().rev() {
                    items.push((ind + 1, mode, c));
                }
            }
        }
    }
}

/// Renders `doc` with `indent` spaces per level and lines of at most `max_width` columns where possible.
pub fn render(doc: &Doc, indent: usize, max_width: usize) -> String {
    let mut out = String::new();
    let mut col = 0usize;
    let mut stack: Vec<(usize, Mode, &Doc)> = vec![(0, Mode::Break, doc)];
    while let Some((ind, mode, d)) = stack.pop() {
        match d {
            Doc::Text(s) => {
                out.push_str(s);
                match s.rfind('\n') {
                    Some(p) => col = width(&s[p + 1..]),
                    None => col += width(s),
                }
            }
            Doc::Line | Doc::Soft | Doc::Hard => {
                let newline = matches!(d, Doc::Hard) || mode == Mode::Break;
                if newline {
                    while out.ends_with(' ') {
                        out.pop();
                    }
                    out.push('\n');
                    out.push_str(&" ".repeat(ind * indent));
                    col = ind * indent;
                } else if matches!(d, Doc::Line) {
                    out.push(' ');
                    col += 1;
                }
            }
            Doc::Group(v) => {
                let flat_ok = {
                    let mut probe: Vec<(usize, Mode, &Doc)> =
                        v.iter().rev().map(|c| (ind, Mode::Flat, c)).collect();
                    fits(max_width as isize - col as isize, &mut probe, &stack)
                };
                let m = if mode == Mode::Flat || flat_ok {
                    Mode::Flat
                } else {
                    Mode::Break
                };
                for c in v.iter().rev() {
                    stack.push((ind, m, c));
                }
            }
            Doc::Indent(v) => {
                for c in v.iter().rev() {
                    stack.push((ind + 1, mode, c));
                }
            }
        }
    }
    out
}
