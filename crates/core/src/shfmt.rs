mod arithm;
mod lexer;
mod nodes;
mod parser;
mod printer;
mod tabwriter;
mod tokens;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Bash,
    Posix,
}

pub fn format(source: &str, lang: Lang, indent: usize) -> Result<String, String> {
    let lang = match lang {
        Lang::Bash => parser::LANG_BASH,
        Lang::Posix => parser::LANG_POSIX,
    };
    let file = parser::parse(source, lang)?;
    let printed = printer::print(&file, indent);
    let tabwidth = if indent == 0 { 8 } else { indent };
    let aligned = tabwriter::align(&printed, tabwidth, indent == 0);
    String::from_utf8(aligned).map_err(|err| err.to_string())
}
