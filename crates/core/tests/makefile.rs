use std::fs;
use std::num::NonZeroUsize;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use libtest_mimic::{Arguments, Failed, Trial};
use makefmt::shfmt::{self, Lang};
use makefmt::{Options, format};

const GENERATED_FILES: u64 = 700;
const FUZZ_INPUTS: u64 = 5000;
const LAYOUTS: [(usize, usize); 4] = [(0, 1), (1, 2), (2, 1), (4, 3)];
const BLANKS: [char; 2] = [' ', '\t'];

const NAMES: [&str; 12] = [
    "$(n)_dyn",
    "CFLAGS",
    "LDFLAGS",
    "foo",
    "foo_bar",
    "x.y",
    "my-var",
    "Ünï",
    "v1",
    "SOME_REALLY_LONG_VARIABLE_NAME",
    "n",
    "VPATH",
];
const BASE_NAMES: [&str; 6] = ["A", "B", "C", "CC", "SRC", "OBJ"];
const LITERALS: [&str; 7] = ["1", "x y", "a.c b.c", "yes", "日本", "z.c", "a.o  b.o"];
const OPS: [&str; 7] = ["=", ":=", "::=", ":::=", "?=", "+=", "!="];
const DEFINE_OPS: [&str; 7] = ["", "=", ":=", "::=", ":::=", "+=", "?="];
const MODIFIERS: [&[&str]; 13] = [
    &["export"],
    &["override"],
    &["private"],
    &["unexport"],
    &["override", "export"],
    &["export", "override"],
    &["private", "export"],
    &["override", "private"],
    &["unexport", "override"],
    &["override", "unexport"],
    &["export", "private"],
    &["private", "export", "override"],
    &["override", "private", "export"],
];
const VALUES: [&str; 37] = [
    "=",
    "==x",
    ":x",
    "a\\\\",
    "a$",
    "1",
    "hello world",
    "$(A)",
    "${B}",
    "$(CC) -O2",
    "$(SRC:.c=.o)",
    "$(SRC:%.c=%.o)",
    "$(patsubst %.c,%.o,$(SRC))",
    "$(subst :,=,a:b)",
    "a:b",
    "x=y",
    "a := b",
    "$(if $(A),yes,no)",
    "$(foreach v,a b,$(v).o)",
    "$(call F,1,2)",
    "$(shell echo hi)",
    "$$HOME",
    "\\#notcomment",
    "'quoted  value'",
    "\"dq\"",
    "日本語",
    "émoji🎉",
    "-Wall -Wextra",
    "$(wildcard *.c)",
    "$(strip  a   b )",
    "$(eval Z = 1)",
    "$(info info-in-value)",
    "a;b",
    "a|b",
    "100%",
    "$(A:a=b)",
    "$(origin A)",
];
const SHELL_VALUES: [&str; 5] = [
    "echo hi",
    "printf '%s' x",
    "echo $(A)",
    "echo a=b c,d",
    "echo '#'",
];
const TARGETS: [&str; 10] = [
    "all",
    "build",
    "clean",
    "out/x.o",
    "test-run",
    "dir/日本.txt",
    "install",
    "lib.a",
    "$(patsubst %.c,%.o,m.c)",
    "$(subst =,-,k=v)",
];
const PREREQS: [&str; 10] = [
    "$(SRC:a=b)",
    "a.c",
    "b.c",
    "$(SRC)",
    "dep",
    "$(OBJ:.o=.c)",
    "src/x.c",
    "ü.h",
    "$(wildcard *.h)",
    "$$(SECOND)",
];
const PATTERNS: [&str; 4] = ["%.o", "build/%.x", "%.d", "all"];
const COMMANDS: [&str; 24] = [
    "if [ -f x ]; then  echo  y; fi",
    "for f in a b; do echo $$f; done",
    "test -n \"$(A)\"  ||  exit 1",
    "echo   $$(( 1+2 ))",
    "case $(A) in a) echo a;; *) echo b;; esac",
    "$(Q)echo  quiet",
    "{ echo a;  echo b; }  >  /dev/null",
    "echo a |  tr a b",
    "echo hi",
    "@echo $@",
    "-rm -f x",
    "@printf '%s\\n' \"$(A)\"",
    "true",
    "echo a  b   c",
    "echo 'x # y'",
    "echo \"$$HOME\"",
    "echo a; echo b",
    "cd x && ls",
    "echo a=b",
    "@echo $(subst a,b,abc):x",
    "echo ünïcödé",
    "+echo plus",
    "$(info in-recipe)",
    "echo $<   $^",
];
const ARGS: [&str; 8] = [
    "$(A)",
    "1",
    "",
    "$(strip $(B))",
    "x y",
    "$(filter a,$(C))",
    "$(origin A)",
    "yes",
];
const COMMENTS: [&str; 8] = [
    "# comment",
    "#",
    "## doc ##",
    "#no space",
    "# ünïcödé ✓",
    "# a = b : c",
    "#\ttabbed",
    "# ifeq (a,b)",
];
const FUNCTION_LINES: [&str; 8] = [
    "$(eval X := 1)",
    "$(foreach v,a b,$(eval V_$(v) := $(v)))",
    "$(info A is $(A))",
    "$(warning careful)",
    "$(if $(A),$(eval HAS_A := 1))",
    "$(call F,1)",
    "$(eval all: TV = 2)",
    "$(eval $(subst :,=,K:1))",
];
const DEFINE_BODY: [&str; 13] = [
    "line one",
    "  indented",
    "\ttabbed $(A)",
    "# not a comment",
    "ifeq (a,b)",
    "endif",
    "$(1) := 3",
    "   ",
    "",
    "trailing  ",
    "a = b \\",
    "x: y ; z",
    "ünïcödé = ✓",
];
const FRAGMENTS: [&str; 64] = [
    "A = 1",
    "A:=1",
    "export A ?= 2",
    "override private B += x  ",
    "unexport C != echo",
    "define X",
    "define",
    "endef",
    "  endef # c",
    "\tendef",
    "ifeq ($(A),1)",
    "ifneq \"a\" 'b'",
    "ifdef A",
    "ifndef",
    "else",
    "else ifeq (a,b)",
    "else  ifdef",
    "endif",
    "  endif # x",
    "all: b c | d",
    "a b &: c",
    "x:: y",
    "%.o: %.c ; cc",
    "$(OBJ): %.o: %.c",
    "t: V = 1",
    "\techo hi",
    "\t",
    "\t  ",
    "   ",
    "",
    "\\",
    "\\\\",
    "a \\",
    "$\\",
    "x = $",
    "$(",
    "))",
    "${",
    "#",
    "# c \\",
    "\\#",
    ".RECIPEPREFIX = >",
    ".RECIPEPREFIX :=",
    ">echo",
    ".POSIX:",
    ".ONESHELL:",
    "include a.mk",
    "-include  b.mk",
    "vpath %.c src",
    ":",
    "=",
    "::=",
    ":::=",
    "&:",
    ";",
    "a;b",
    "|",
    "\r",
    "é = ü",
    "日本語: 語",
    "$(eval A = 1)",
    "x := $(foo:a=b) # c",
    "private",
    "export",
];
const LINE_ENDING_CASES: [(&str, &str, &str); 16] = [
    (
        "missing_final_newline",
        "A=1\nall:\n\techo",
        "A = 1\nall:\n\techo\n",
    ),
    ("continued_assignment_at_eof", "A = a \\", "A = a \\"),
    ("continued_rule_at_eof", "all: x \\", "all: x \\"),
    (
        "lone_carriage_return",
        "A = 1\r\nB = x\ry  \r\n",
        "A = 1\r\nB = x\ry  \r\n",
    ),
    ("lone_carriage_return_at_eof", "A=1\r", "A=1\r"),
    ("lf_then_crlf", "A=1\nB=2\r\n", "A = 1\nB = 2\n"),
    ("crlf_then_lf", "A=1\r\nB=2\n", "A = 1\r\nB = 2\r\n"),
    ("whitespace_only", "  \n\t\n\n", "\t\n"),
    ("blank_lines_only", "\n\n\n", ""),
    ("tab_only_recipe_line", "all:\n\t\n", "all:\n\t\n"),
    (
        "crlf_trailing_whitespace",
        "A = 1  \r\nall:\r\n\techo  \r\n",
        "A = 1  \r\nall:\r\n\techo\r\n",
    ),
    ("tab_indented_assignment", "\tA = 1\n", "\tA = 1\n"),
    (
        "utf8_bom",
        "\u{FEFF}X:=1\nLONG:=2\n",
        "\u{FEFF}X    := 1\nLONG := 2\n",
    ),
    (
        "utf8_bom_crlf",
        "\u{FEFF}X:=1\r\nLONG:=2\r\n",
        "\u{FEFF}X    := 1\r\nLONG := 2\r\n",
    ),
    ("utf8_bom_only", "\u{FEFF}\n\n", "\u{FEFF}"),
    (
        "utf8_double_bom",
        "\u{FEFF}\u{FEFF}X:=1\nLONG:=2\n",
        "\u{FEFF}\u{FEFF}X   := 1\nLONG := 2\n",
    ),
];
const UNCHANGED_CASES: [(&str, &str); 17] = [
    (
        "recipes_other_shell",
        "SHELL = /bin/dash\nall:\n\techo   a\n",
    ),
    (
        "recipes_mixed_shells",
        "SHELL = /bin/sh\nall: SHELL = /bin/bash\nall:\n\techo   a\n",
    ),
    (
        "recipes_shell_reference",
        "SHELL = $(firstword /bin/sh)\nall:\n\techo   a\n",
    ),
    ("recipes_shell_append", "SHELL += -e\nall:\n\techo   a\n"),
    (
        "recipes_oneshell",
        ".ONESHELL:\nall:\n\tif true; then\n\t  echo   a\n\tfi\n",
    ),
    ("recipes_conditional_word", "all:\n\tifeq   (a,b)\n"),
    (
        "recipes_comment_continuation",
        "all:\n\techo   a # note \\\n\techo  b\n",
    ),
    (
        "recipe_prefix_in_false_conditional",
        "ifdef MAKEFMT_UNDEFINED\n.RECIPEPREFIX = >\nendif\nall:\n\t@echo   hi\nA=1\n",
    ),
    (
        "recipe_prefix_in_true_conditional",
        "ifndef MAKEFMT_UNDEFINED\n.RECIPEPREFIX = >\nendif\nall:\n>@echo   hi\nA=1\n",
    ),
    (
        "recipe_prefix_in_tab_indented_conditional",
        "\tifdef MAKEFMT_UNDEFINED\n.RECIPEPREFIX = >\n\tendif\nall:\n\t@echo   hi\nA=1\n",
    ),
    (
        "recipe_prefix_conditional_assignment",
        ".RECIPEPREFIX ?= >\nall:\n\t@echo   hi\nA=1\n",
    ),
    (
        "recipe_prefix_from_variable",
        ".RECIPEPREFIX := $(MAKEFMT_UNDEFINED)\nall:\n\t@echo   hi\nA=1\n",
    ),
    (
        "recipe_prefix_define",
        "override define .RECIPEPREFIX\n>\nendef\nall:\n>@echo   hi\nA=1\n",
    ),
    (
        "recipe_prefix_eval",
        "$(eval .RECIPEPREFIX = >)\nall:\n>@echo   hi\nA=1\n",
    ),
    (
        "recipe_prefix_target_specific",
        "all: .RECIPEPREFIX = >\nall:\n\t@echo   hi\nA=1\n",
    ),
    (
        "recipe_prefix_multibyte",
        ".RECIPEPREFIX = \u{e9}\nall:\n\u{e9}@echo   hi\nA=1\n",
    ),
    (
        "recipe_prefix_reference",
        "B = $(.RECIPEPREFIX)\nall:\n\t@echo   hi\nA=1\n",
    ),
];
const CHARS: [char; 32] = [
    ' ', ' ', '\t', '\\', '$', '(', ')', '{', '}', ':', '=', '#', ';', '|', '&', '%', '?', '+',
    '!', '\r', '\'', '"', 'a', 'b', 'x', 'é', '日', '>', '@', '-', '.', ',',
];

enum Kept {
    Recipe(String),
    Define(String),
}

struct Generated {
    source: String,
    kept: Vec<Kept>,
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

struct Generator {
    rng: Rng,
    lines: Vec<String>,
    kept: Vec<Kept>,
    prefix: char,
    groups: usize,
}

impl Generator {
    fn ws(&mut self) -> &'static str {
        self.rng.pick(&["", "", " ", " ", "  ", "\t", " \t"])
    }

    fn ws1(&mut self) -> &'static str {
        self.rng.pick(&[" ", " ", " ", "  ", "\t", " \t ", "   "])
    }

    fn indent(&mut self) -> &'static str {
        self.rng.pick(&["", "", "", "", "", " ", "  ", "    "])
    }

    fn trailing(&mut self) -> &'static str {
        self.rng
            .pick(&["", "", "", "", "", "", " ", "  ", "\t", " \t"])
    }

    fn modifiers(&mut self) -> String {
        if self.rng.chance(60) {
            return String::new();
        }
        let mut out = String::new();
        for word in self.rng.pick(&MODIFIERS) {
            out.push_str(word);
            out.push_str(self.ws1());
        }
        out
    }

    fn value(&mut self) -> Vec<String> {
        let count = if self.rng.chance(3) {
            40
        } else {
            self.rng.below(5)
        };
        (0..count)
            .map(|_| self.rng.pick(&VALUES).to_string())
            .collect()
    }

    fn statement(&mut self, depth: usize) {
        match self.rng.below(18) {
            0..=3 => self.assignment(),
            4 | 5 => self.rule(),
            6 if depth < 3 => self.conditional(depth),
            6 => self.comment(),
            7 => self.define(depth),
            8 => self.comment(),
            9 => self.blank(),
            10 => self.include(),
            11 => self.vpath(),
            12 => self.directive(),
            13 => self.target_variable(),
            14 => self.function(),
            15 if depth == 0 => self.recipe_prefix(),
            15 => self.assignment(),
            16 if depth < 3 => self.conditional_recipes(),
            16 => self.rule(),
            17 => self.tabbed(),
            _ => unreachable!(),
        }
    }

    fn continued(&mut self, head: String, words: &[String]) {
        if !words.is_empty() && self.rng.chance(20) {
            let mut line = head;
            if self.rng.chance(25) {
                line.push('\\');
                self.push(line);
                line = self.rng.pick(&["", "  ", "\t"]).to_string();
            }
            for (k, word) in words.iter().enumerate() {
                line.push_str(word);
                if k + 1 == words.len() {
                    line.push_str(self.trailing());
                    self.push(line);
                    return;
                }
                if self.rng.chance(50) {
                    line.push_str(self.ws());
                    line.push('\\');
                    self.push(line);
                    if self.rng.chance(10) {
                        self.push("  \\".to_string());
                    }
                    line = self.rng.pick(&["", " ", "  ", "\t", "      "]).to_string();
                } else {
                    line.push_str(self.ws1());
                }
            }
            return;
        }
        let mut line = head;
        for (k, word) in words.iter().enumerate() {
            if k > 0 {
                line.push_str(self.ws1());
            }
            line.push_str(word);
        }
        if self.rng.chance(10) {
            line.push_str(self.ws1());
            line.push_str("# trailing note");
        }
        line.push_str(self.trailing());
        self.push(line);
    }

    fn assignment(&mut self) {
        let modifiers = self.modifiers();
        let base = self.rng.chance(30);
        let name = if base {
            self.rng.pick(&BASE_NAMES)
        } else {
            self.rng.pick(&NAMES)
        };
        let op = self.rng.pick(&OPS);
        let words = if op == "!=" {
            vec![self.rng.pick(&SHELL_VALUES).to_string()]
        } else if base {
            (0..self.rng.below(3))
                .map(|_| self.rng.pick(&LITERALS).to_string())
                .collect()
        } else {
            self.value()
        };
        let head = format!(
            "{}{modifiers}{name}{}{op}{}",
            self.indent(),
            self.ws(),
            self.ws()
        );
        self.continued(head, &words);
    }

    fn target_variable(&mut self) {
        let target = self.rng.pick(&PATTERNS);
        let modifiers = self.modifiers();
        let name = self.rng.pick(&NAMES);
        let op = self.rng.pick(&OPS[..6]);
        let words = self.value();
        let head = format!(
            "{}{target}{}:{}{modifiers}{name}{}{op}{}",
            self.indent(),
            self.ws(),
            self.ws1(),
            self.ws(),
            self.ws()
        );
        self.continued(head, &words);
    }

    fn rule(&mut self) {
        let (head, needs_recipe, phony) = match self.rng.below(7) {
            0 | 1 => (self.targets(TARGETS.len(), ":"), false, false),
            2 => {
                let target = self.rng.pick(&["dc1", "dc2"]);
                (format!("{target}{}::", self.ws()), false, false)
            }
            3 => {
                self.groups += 1;
                let n = self.groups;
                let separator = self.rng.pick(&["&:", "&::"]);
                (
                    format!("g{n}a{}g{n}b{}{separator}", self.ws1(), self.ws()),
                    true,
                    false,
                )
            }
            4 => (
                format!(
                    "s1.o{}s2.o{}:{}%.o{}:",
                    self.ws1(),
                    self.ws(),
                    self.ws(),
                    self.ws()
                ),
                false,
                false,
            ),
            5 => {
                let pattern = self.rng.pick(&["%.o", "build/%.x", "%.d"]);
                (format!("{pattern}{}:", self.ws()), false, false)
            }
            6 => (format!(".PHONY{}:", self.ws()), false, true),
            _ => unreachable!(),
        };
        let head = format!("{}{head}{}", self.indent(), self.ws());
        let mut words: Vec<String> = (0..self.rng.below(4))
            .map(|_| self.rng.pick(&PREREQS).to_string())
            .collect();
        if head.contains('%') && !head.contains("s1.o") {
            words.insert(0, "%.c".to_string());
        }
        if head.contains("s1.o") {
            words.insert(0, "%.c".to_string());
        }
        if self.rng.chance(20) {
            words.push("|".to_string());
            words.push("order".to_string());
        }
        if phony {
            words = vec!["all".to_string(), "clean".to_string()];
        }
        if !phony && (needs_recipe || self.rng.chance(15)) && self.rng.chance(50) {
            let mut line = head;
            line.push_str(&words.join(" "));
            line.push_str(self.ws());
            line.push(';');
            line.push_str(self.ws());
            line.push_str(self.rng.pick(&COMMANDS));
            line.push_str(self.trailing());
            self.push(line);
            return;
        }
        self.continued(head, &words);
        if phony {
            return;
        }
        let count = if needs_recipe {
            1 + self.rng.below(3)
        } else {
            self.rng.below(4)
        };
        for _ in 0..count {
            self.recipe();
        }
    }

    fn targets(&mut self, limit: usize, separator: &str) -> String {
        let count = 1 + self.rng.below(3);
        let mut out = String::new();
        for k in 0..count {
            if k > 0 {
                out.push_str(self.ws1());
            }
            out.push_str(self.rng.pick(&TARGETS[..limit]));
        }
        out.push_str(self.ws());
        out.push_str(separator);
        out
    }

    fn recipe(&mut self) {
        if self.rng.chance(8) {
            self.lines.push(String::new());
        }
        if self.rng.chance(8) {
            let comment = self.rng.pick(&COMMENTS).to_string();
            self.lines.push(comment);
        }
        if self.rng.chance(5) {
            let line = format!("{}{}", self.prefix, self.rng.pick(&["", "  ", "\t"]));
            self.keep_recipe(line);
            return;
        }
        let mut line = format!(
            "{}{}{}",
            self.prefix,
            self.rng.pick(&["", "", "", " ", "\t", "  "]),
            self.rng.pick(&COMMANDS)
        );
        if self.rng.chance(15) {
            line.push_str(self.ws1());
            line.push('\\');
            self.keep_recipe(line);
            let start = match self.rng.below(3) {
                0 => self.prefix.to_string(),
                1 => String::new(),
                2 => "  ".to_string(),
                _ => unreachable!(),
            };
            line = format!("{start}{}", self.rng.pick(&COMMANDS));
        }
        line.push_str(self.trailing());
        self.keep_recipe(line);
    }

    fn keep_recipe(&mut self, line: String) {
        self.kept.push(Kept::Recipe(line.clone()));
        self.push(line);
    }

    fn condition(&mut self) -> String {
        if self.rng.chance(30) {
            let keyword = self.rng.pick(&["ifdef", "ifndef"]);
            let name = self.rng.pick(&[&NAMES[1..], &BASE_NAMES[..]].concat());
            return format!("{keyword}{}{name}", self.ws1());
        }
        let keyword = self.rng.pick(&["ifeq", "ifneq"]);
        let a = self.rng.pick(&ARGS);
        let b = self.rng.pick(&ARGS);
        let args = match self.rng.below(5) {
            0 | 1 => format!(
                "({}{a}{},{}{b}{})",
                self.rng.pick(&["", " "]),
                self.rng.pick(&["", " "]),
                self.rng.pick(&["", " ", " \\\n", "\\\n    "]),
                self.rng.pick(&["", " "])
            ),
            2 => format!("\"{a}\"{}\"{b}\"", self.ws1()),
            3 => format!("'{a}'{}'{b}'", self.ws1()),
            4 => format!("\"{a}\"{}'{b}'", self.ws1()),
            _ => unreachable!(),
        };
        format!("{keyword}{}{args}", self.ws1())
    }

    fn conditional(&mut self, depth: usize) {
        let condition = self.condition();
        let line = format!("{}{condition}{}", self.indent(), self.trailing());
        self.push(line);
        self.body(depth);
        for _ in 0..self.rng.below(3) {
            let condition = self.condition();
            let line = format!("{}else{}{condition}", self.indent(), self.ws1());
            self.push(line);
            self.body(depth);
        }
        if self.rng.chance(50) {
            let line = format!("{}else{}", self.indent(), self.trailing());
            self.push(line);
            self.body(depth);
        }
        let comment = if self.rng.chance(20) { " # end" } else { "" };
        let line = format!("{}endif{comment}{}", self.indent(), self.trailing());
        self.push(line);
    }

    fn body(&mut self, depth: usize) {
        for _ in 0..self.rng.below(4) {
            self.statement(depth + 1);
        }
    }

    fn conditional_recipes(&mut self) {
        let target = self.targets(TARGETS.len(), ":");
        let line = format!("{target}{}", self.trailing());
        self.push(line);
        let condition = self.condition();
        let line = format!("{}{condition}", self.indent());
        self.push(line);
        for _ in 0..1 + self.rng.below(2) {
            self.recipe();
        }
        if self.rng.chance(50) {
            let line = format!("{}else", self.indent());
            self.push(line);
            for _ in 0..1 + self.rng.below(2) {
                self.recipe();
            }
        }
        let line = format!("{}endif", self.indent());
        self.push(line);
    }

    fn define(&mut self, depth: usize) {
        let modifiers = self.modifiers();
        let name = self.rng.pick(&NAMES);
        let op = self.rng.pick(&DEFINE_OPS);
        let op = if op.is_empty() {
            String::new()
        } else {
            format!("{}{op}", self.ws())
        };
        let line = format!(
            "{}{modifiers}define{}{name}{op}{}",
            self.indent(),
            self.ws1(),
            self.trailing()
        );
        self.push(line);
        self.define_body(depth == 0);
        let comment = if self.rng.chance(20) { " # done" } else { "" };
        let line = format!("{}endef{comment}{}", self.indent(), self.trailing());
        self.push(line);
    }

    fn define_body(&mut self, top: bool) {
        for _ in 0..self.rng.below(5) {
            if top && self.rng.chance(10) {
                let line = format!("{}define{}INNER", self.indent(), self.ws1());
                self.keep_define(line);
                self.define_body(false);
                let line = format!("{}endef", self.indent());
                self.keep_define(line);
                continue;
            }
            if top && self.rng.chance(5) {
                self.keep_define(format!("{}endef", self.prefix));
                continue;
            }
            let line = self.rng.pick(&DEFINE_BODY).to_string();
            let continued = line.ends_with('\\');
            self.keep_define(line);
            if continued {
                self.keep_define("  continued".to_string());
            }
        }
    }

    fn keep_define(&mut self, line: String) {
        self.kept.push(Kept::Define(line.clone()));
        self.push(line);
    }

    fn comment(&mut self) {
        let mut line = format!("{}{}", self.indent(), self.rng.pick(&COMMENTS));
        if self.rng.chance(10) {
            line.push_str(" \\");
            self.push(line);
            line = format!("{}continued comment", self.rng.pick(&["", "  ", "\t"]));
        }
        line.push_str(self.trailing());
        self.push(line);
    }

    fn blank(&mut self) {
        for _ in 0..1 + self.rng.below(3) {
            let line = self
                .rng
                .pick(&["", "", "", "   ", "\t", " \t "])
                .to_string();
            self.push(line);
        }
    }

    fn include(&mut self) {
        let keyword = self.rng.pick(&["include", "-include", "sinclude"]);
        let files = if keyword == "include" {
            "common.mk"
        } else {
            self.rng.pick(&[
                "missing.mk",
                "common.mk",
                "missing.mk  nope/x.mk",
                "$(wildcard none.mk)",
            ])
        };
        let comment = if self.rng.chance(20) { " # inc" } else { "" };
        let line = format!(
            "{}{keyword}{}{files}{comment}{}",
            self.indent(),
            self.ws1(),
            self.trailing()
        );
        self.push(line);
    }

    fn vpath(&mut self) {
        let rest = self
            .rng
            .pick(&["%.c src", "%.h  inc:lib", "%.c", "", "% $(A)"]);
        let line = if rest.is_empty() {
            format!("{}vpath{}", self.indent(), self.trailing())
        } else {
            format!(
                "{}vpath{}{rest}{}",
                self.indent(),
                self.ws1(),
                self.trailing()
            )
        };
        self.push(line);
    }

    fn tabbed(&mut self) {
        let text = self.rng.pick(&[
            "CFLAGS += -g",
            "foo := 1  ",
            "export A",
            "-include missing.mk",
            "LDFLAGS = a \\",
        ]);
        let line = format!("\t{text}");
        let continued = line.ends_with('\\');
        if self.prefix == '\t' {
            self.keep_recipe(line);
        } else {
            self.push(line);
        }
        if continued {
            self.lines.push("  b".to_string());
        }
    }

    fn directive(&mut self) {
        let rest = match self.rng.below(6) {
            0 => format!("export{}A{}B", self.ws1(), self.ws1()),
            1 => "export".to_string(),
            2 => format!("unexport{}C{}foo", self.ws1(), self.ws1()),
            3 => format!("undefine{}{}", self.ws1(), self.rng.pick(&NAMES)),
            4 => format!("override{}undefine{}B", self.ws1(), self.ws1()),
            5 => format!("export{}A{}\\\n{}B", self.ws1(), self.ws1(), self.indent()),
            _ => unreachable!(),
        };
        let line = format!("{}{rest}{}", self.indent(), self.trailing());
        self.push(line);
    }

    fn push(&mut self, text: String) {
        for line in text.split('\n') {
            self.lines.push(line.to_string());
        }
    }

    fn function(&mut self) {
        let line = format!(
            "{}{}{}",
            self.indent(),
            self.rng.pick(&FUNCTION_LINES),
            self.trailing()
        );
        self.push(line);
    }

    fn recipe_prefix(&mut self) {
        match self.rng.below(10) {
            0 => {
                let value = if self.prefix == '\t' { ">" } else { "" };
                self.push(format!(
                    "ifdef MAKEFMT_UNDEFINED\n.RECIPEPREFIX = {value}\nendif"
                ));
                return;
            }
            1 => {
                self.push(".RECIPEPREFIX ?= >".to_string());
                return;
            }
            2 => {
                self.push(".RECIPEPREFIX := $(MAKEFMT_UNDEFINED)".to_string());
                self.prefix = '\t';
                return;
            }
            3 => {
                self.push("$(eval .RECIPEPREFIX = >)".to_string());
                self.prefix = '>';
                return;
            }
            4 => {
                self.push("define .RECIPEPREFIX\n>\nendef".to_string());
                self.prefix = '>';
                return;
            }
            5..=9 => {}
            _ => unreachable!(),
        }
        if self.prefix == '\t' {
            let op = self.rng.pick(&["=", ":="]);
            let line = format!(".RECIPEPREFIX{}{op}{}>", self.ws(), self.ws());
            self.push(line);
            self.prefix = '>';
            return;
        }
        let line = format!(".RECIPEPREFIX{}={}", self.ws(), self.trailing());
        self.push(line);
        self.prefix = '\t';
    }
}

fn generate(seed: u64) -> Generated {
    let mut generator = Generator {
        rng: Rng(seed),
        lines: Vec::new(),
        kept: Vec::new(),
        prefix: '\t',
        groups: 0,
    };
    if generator.rng.chance(10) {
        generator.lines.push(".POSIX:".to_string());
    }
    if generator.rng.chance(10) {
        let line = format!(".ONESHELL{}:", generator.ws());
        generator.lines.push(line);
    }
    for _ in 0..3 + generator.rng.below(25) {
        generator.statement(0);
    }
    let eol = if generator.rng.chance(10) {
        "\r\n"
    } else {
        "\n"
    };
    let mut source = generator.lines.join(eol);
    if !generator.rng.chance(10) {
        source.push_str(eol);
    }
    Generated {
        source,
        kept: generator.kept,
    }
}

fn soup(seed: u64) -> String {
    let mut rng = Rng(seed);
    let mut out = String::new();
    for _ in 0..rng.below(30) {
        if rng.chance(60) {
            out.push_str(rng.pick(&FRAGMENTS));
        } else {
            for _ in 0..rng.below(20) {
                out.push(rng.pick(&CHARS));
            }
        }
        out.push_str(rng.pick(&["\n", "\n", "\n", "\r\n"]));
    }
    if rng.chance(30) {
        out.pop();
    }
    out
}

fn option_matrix() -> Vec<Options> {
    let mut all = Vec::new();
    for bits in 0..64 {
        for (indent_width, max_blank_lines) in LAYOUTS {
            all.push(Options {
                align_across_comments: bits & 16 != 0,
                align_assignments: bits & 1 != 0,
                align_continuations: bits & 2 != 0,
                format_recipes: bits & 32 != 0,
                indent_conditionals: bits & 4 != 0,
                indent_width,
                max_blank_lines: NonZeroUsize::new(max_blank_lines)
                    .expect("layouts keep at least one blank line"),
                trim_trailing_whitespace: bits & 8 != 0,
            });
        }
    }
    all
}

fn label(options: &Options) -> String {
    format!(
        "align_across_comments={},align_assignments={},align_continuations={},format_recipes={},indent_conditionals={},indent_width={},max_blank_lines={},trim_trailing_whitespace={}",
        options.align_across_comments,
        options.align_assignments,
        options.align_continuations,
        options.format_recipes,
        options.indent_conditionals,
        options.indent_width,
        options.max_blank_lines,
        options.trim_trailing_whitespace
    )
}

fn main() {
    let args = Arguments::from_args();
    let mut trials = fixture_trials();
    trials.extend(generated_trials());
    trials.extend(fuzz_trials());
    libtest_mimic::run(&args, trials).exit();
}

fn fixture_trials() -> Vec<Trial> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/makefile");
    let mut names: Vec<String> = fs::read_dir(&dir)
        .expect("tests/makefile fixture directory must exist")
        .map(|entry| entry.expect("fixture directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "mk"))
        .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut trials = Vec::new();
    for name in names {
        let source = read(&dir.join(format!("{name}.mk")));
        let expected = read(&dir.join(format!("{name}.expected")));
        let toml = dir.join(format!("{name}.toml"));
        let options = if toml.exists() {
            Options::from_toml(&read(&toml)).expect("valid fixture options")
        } else {
            Options::default()
        };
        let crlf_source = source.replace('\n', "\r\n");
        let crlf_expected = expected.replace('\n', "\r\n");
        let crlf_options = options.clone();
        trials.push(Trial::test(format!("fixture::{name}::crlf"), move || {
            check_snapshot(&crlf_source, &crlf_expected, &crlf_options)
        }));
        push_fixture(
            &mut trials,
            format!("fixture::{name}"),
            source,
            expected,
            options,
        );
    }
    for (name, source) in UNCHANGED_CASES {
        push_fixture(
            &mut trials,
            format!("unchanged::{name}"),
            source.to_string(),
            source.to_string(),
            Options::default(),
        );
    }
    for (name, source, expected) in LINE_ENDING_CASES {
        push_fixture(
            &mut trials,
            format!("line_endings::{name}"),
            source.to_string(),
            expected.to_string(),
            Options::default(),
        );
    }
    trials
}

fn push_fixture(
    trials: &mut Vec<Trial>,
    name: String,
    source: String,
    expected: String,
    options: Options,
) {
    let snapshot_source = source.clone();
    trials.push(Trial::test(name.clone(), move || {
        check_snapshot(&snapshot_source, &expected, &options)
    }));
    for options in option_matrix() {
        let source = source.clone();
        trials.push(Trial::test(
            format!("{name}::make::{}", label(&options)),
            move || check_case(&source, &options, &[]),
        ));
    }
}

fn generated_trials() -> Vec<Trial> {
    let mut trials = Vec::new();
    for seed in 0..GENERATED_FILES {
        for options in option_matrix() {
            trials.push(Trial::test(
                format!("generated::{seed:04}::{}", label(&options)),
                move || {
                    let generated = generate(seed);
                    check_case(&generated.source, &options, &generated.kept)
                },
            ));
        }
    }
    trials
}

fn fuzz_trials() -> Vec<Trial> {
    let matrix = option_matrix();
    let mut trials = Vec::new();
    for seed in 0..FUZZ_INPUTS {
        for options in [
            Options::default(),
            matrix[seed as usize % matrix.len()].clone(),
        ] {
            trials.push(Trial::test(
                format!("fuzz::{seed:04}::{}", label(&options)),
                move || {
                    let source = soup(seed);
                    check_idempotent(&source, &options).map(|_| ())
                },
            ));
        }
    }
    trials
}

fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn check_snapshot(source: &str, expected: &str, options: &Options) -> Result<(), Failed> {
    let output = check_idempotent(source, options)?;
    if output != expected {
        return Err(format!(
            "snapshot mismatch\n{}\n--- expected ---\n{expected}\n--- actual ---\n{output}",
            first_difference(expected, &output)
        )
        .into());
    }
    Ok(())
}

fn check_idempotent(source: &str, options: &Options) -> Result<String, Failed> {
    let once = format(source, options);
    let twice = format(&once, options);
    if twice != once {
        return Err(format!(
            "not idempotent\n{}\n--- input ---\n{source:?}\n--- first pass ---\n{once:?}\n--- second pass ---\n{twice:?}",
            first_difference(&once, &twice)
        )
        .into());
    }
    Ok(once)
}

fn check_case(source: &str, options: &Options, kept: &[Kept]) -> Result<(), Failed> {
    let output = check_idempotent(source, options)?;
    let mut lines = output.lines();
    for item in kept {
        let found = match item {
            Kept::Recipe(_) if options.format_recipes => continue,
            Kept::Recipe(line) => {
                lines.any(|out| out.trim_end_matches(BLANKS) == line.trim_end_matches(BLANKS))
            }
            Kept::Define(line) => lines.any(|out| out == line),
        };
        if !found {
            let line = match item {
                Kept::Recipe(line) => format!("recipe line {line:?}"),
                Kept::Define(line) => format!("define body line {line:?}"),
            };
            return Err(format!(
                "{line} was not preserved\n--- input ---\n{source:?}\n--- output ---\n{output:?}"
            )
            .into());
        }
    }
    if options.trim_trailing_whitespace {
        let untrimmed = format(
            source,
            &Options {
                trim_trailing_whitespace: false,
                ..options.clone()
            },
        );
        return check_only_trimmed(source, &untrimmed, &output);
    }
    let (before, after) = make_databases(source, &output, options.format_recipes)?;
    if before != after {
        return Err(format!(
            "make database changed\n{}\n--- input ---\n{source}\n--- output ---\n{output}\n--- input (escaped) ---\n{source:?}",
            first_difference(&before, &after)
        )
        .into());
    }
    Ok(())
}

fn check_only_trimmed(source: &str, untrimmed: &str, trimmed: &str) -> Result<(), Failed> {
    let same_shape = untrimmed.lines().count() == trimmed.lines().count()
        && untrimmed.ends_with('\n') == trimmed.ends_with('\n');
    let only_trimmed = untrimmed
        .lines()
        .zip(trimmed.lines())
        .all(|(before, after)| {
            before == after
                || (before.trim_end_matches(BLANKS) == after && !after.ends_with(['\\', '$']))
        });
    if same_shape && only_trimmed {
        return Ok(());
    }
    Err(format!(
        "trimming changed more than trailing whitespace\n{}\n--- input ---\n{source:?}",
        first_difference(untrimmed, trimmed)
    )
    .into())
}

fn first_difference(a: &str, b: &str) -> String {
    let a: Vec<&str> = a.lines().collect();
    let b: Vec<&str> = b.lines().collect();
    let mut index = 0;
    while index < a.len() && index < b.len() && a[index] == b[index] {
        index += 1;
    }
    let start = index.saturating_sub(3);
    format!(
        "first difference at line {}\n--- before ---\n{:#?}\n--- after ---\n{:#?}",
        index + 1,
        &a[start..(index + 4).min(a.len())],
        &b[start..(index + 4).min(b.len())]
    )
}

fn make_databases(source: &str, output: &str, shell: bool) -> Result<(String, String), Failed> {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "makefmt-tests-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir)?;
    let before = make_database(&dir, source, shell);
    let after = make_database(&dir, output, shell);
    fs::remove_dir_all(&dir)?;
    let (before, status) = before?;
    if !status {
        return Err(format!("make rejects the input itself\n--- input ---\n{source}\n--- make ---\n{before}\n--- input (escaped) ---\n{source:?}").into());
    }
    Ok((before, after?.0))
}

fn make_database(dir: &Path, source: &str, shell: bool) -> Result<(String, bool), Failed> {
    fs::write(dir.join("common.mk"), "")?;
    fs::write(dir.join("dump.mk"), "__dump__: ; @env | LC_ALL=C sort\n")?;
    fs::write(dir.join("Makefile"), source)?;
    let path = std::env::var("PATH").map_err(|_| "PATH is not set, so GNU make cannot be found")?;
    let output = Command::new("make")
        .args([
            "-p",
            "-r",
            "-R",
            "--no-print-directory",
            "-f",
            "Makefile",
            "-f",
            "dump.mk",
            "__dump__",
        ])
        .current_dir(dir)
        .env_clear()
        .env("PATH", path)
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| {
            format!("cannot run `make` ({error}); GNU make must be on PATH for the semantic tests")
        })?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str("--- stderr ---\n");
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text.push_str(&format!("--- status {} ---\n", output.status));
    Ok((normalize(&text, source, shell), output.status.success()))
}

fn normalize(text: &str, source: &str, shell: bool) -> String {
    let ignored = [
        "# Make data base, printed on",
        "# Finished Make data base on",
        "#  Last modified",
        "# Load=",
        "# strcache",
        "# current buf",
    ];
    let mut out = String::new();
    let mut recipe = false;
    let mut prefix = '\t';
    let mut logical: Vec<&str> = Vec::new();
    for line in text.lines() {
        if ignored.iter().any(|prefix| line.starts_with(prefix))
            || line.contains("hash-table stats")
        {
            continue;
        }
        if recipe && shell && !line.is_empty() {
            logical.push(line);
            if !continued(line) {
                out.push_str(&canonical_recipe(&logical, prefix));
                out.push('\n');
                logical.clear();
            }
            continue;
        }
        if recipe && !line.is_empty() {
            let mut chars = line.chars();
            out.extend(chars.next());
            out.push_str(chars.as_str().trim_matches(BLANKS));
            out.push('\n');
            continue;
        }
        recipe = line.starts_with("#  recipe to execute");
        if recipe {
            prefix = recipe_prefix(line, source);
        }
        out.push_str(&strip_locations(line));
        out.push('\n');
    }
    out
}

fn strip_locations(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(index) = rest.find("Makefile") {
        out.push_str(&rest[..index + "Makefile".len()]);
        rest = &rest[index + "Makefile".len()..];
        let Some(after) = rest
            .strip_prefix("', line ")
            .or_else(|| rest.strip_prefix(':'))
        else {
            continue;
        };
        let digits = after.len() - after.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 {
            out.push_str(":LINE");
            rest = &after[digits..];
        }
    }
    out.push_str(rest);
    out
}

fn continued(line: &str) -> bool {
    (line.len() - line.trim_end_matches('\\').len()) % 2 == 1
}

fn recipe_prefix(line: &str, source: &str) -> char {
    let location = line
        .strip_prefix("#  recipe to execute (from '")
        .and_then(|rest| rest.strip_suffix("):"))
        .unwrap_or_else(|| panic!("unexpected recipe header {line:?}"));
    let (file, number) = location
        .split_once("', line ")
        .unwrap_or_else(|| panic!("unexpected recipe location {location:?}"));
    match file {
        "Makefile" => {
            let number: usize = number.parse().expect("recipe line number");
            source
                .lines()
                .nth(number - 1)
                .and_then(|line| line.chars().next())
                .expect("recipe line exists in the Makefile")
        }
        "dump.mk" => '\t',
        _ => panic!("unexpected recipe file {file:?}"),
    }
}

fn canonical_recipe(lines: &[&str], prefix: char) -> String {
    let mut chars = lines[0].chars();
    let Some(printed) = chars.next() else {
        unreachable!("make prints recipe lines with the recipe prefix");
    };
    let first = chars.as_str();
    let body = first.trim_start_matches([' ', '\t', '@', '-', '+']);
    let flags: String = first[..first.len() - body.len()]
        .chars()
        .filter(|c| !BLANKS.contains(c))
        .collect();
    let mut text = body.to_string();
    for line in &lines[1..] {
        text.push('\n');
        text.push_str(line.strip_prefix(prefix).unwrap_or(line));
    }
    let Some((shell, refs)) = shell_text(&text) else {
        return format!("{printed}{flags} raw {:?}", text.trim_matches(BLANKS));
    };
    match shfmt::format(&shell, Lang::Bash, 0) {
        Ok(formatted) => format!("{printed}{flags} shell {formatted:?} {refs:?}"),
        Err(_) => format!("{printed}{flags} raw {:?}", text.trim_matches(BLANKS)),
    }
}

fn shell_text(text: &str) -> Option<(String, Vec<String>)> {
    let mut shell = String::new();
    let mut refs = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '$' {
            shell.push(c);
            continue;
        }
        let next = chars.next()?;
        let mut reference = format!("${next}");
        match next {
            '$' => {
                shell.push('$');
                continue;
            }
            '(' | '{' => {
                let close = if next == '(' { ')' } else { '}' };
                let mut depth = 1;
                while depth > 0 {
                    let c = chars.next()?;
                    if c == next {
                        depth += 1;
                    } else if c == close {
                        depth -= 1;
                    } else if c == '\n' {
                        return None;
                    }
                    reference.push(c);
                }
            }
            ' ' | '\t' | '\n' | '\\' => return None,
            _ => {}
        }
        shell.push_str(&format!("__ref{}__", refs.len()));
        refs.push(reference);
    }
    Some((shell, refs))
}
