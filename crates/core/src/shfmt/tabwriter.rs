use super::printer::ESCAPE;

#[derive(Clone, Copy, Default)]
struct Cell {
    size: usize,
    width: usize,
    htab: bool,
}

struct Writer {
    output: Vec<u8>,
    tabwidth: usize,
    tab_indent: bool,
    buf: Vec<u8>,
    pos: usize,
    cell: Cell,
    escaped: bool,
    lines: Vec<Vec<Cell>>,
    widths: Vec<usize>,
}

pub fn align(input: &[u8], tabwidth: usize, tab_indent: bool) -> Vec<u8> {
    let mut w = Writer {
        output: Vec::new(),
        tabwidth,
        tab_indent,
        buf: Vec::new(),
        pos: 0,
        cell: Cell::default(),
        escaped: false,
        lines: vec![Vec::new()],
        widths: Vec::new(),
    };
    w.write(input);
    w.flush();
    w.output
}

fn rune_count(bytes: &[u8]) -> usize {
    let mut count = 0;
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => return count + s.chars().count(),
            Err(e) => {
                let valid = e.valid_up_to();
                count += std::str::from_utf8(&rest[..valid])
                    .map(|s| s.chars().count())
                    .unwrap_or(0)
                    + 1;
                rest = &rest[valid + 1..];
            }
        }
    }
}

impl Writer {
    fn reset(&mut self) {
        self.buf.clear();
        self.pos = 0;
        self.cell = Cell::default();
        self.escaped = false;
        self.lines = vec![Vec::new()];
        self.widths.clear();
    }

    fn write_padding(&mut self, textw: usize, cellw: usize, use_tabs: bool) {
        if use_tabs {
            if self.tabwidth == 0 {
                return;
            }
            let cellw = cellw.div_ceil(self.tabwidth) * self.tabwidth;
            let n = cellw - textw;
            for _ in 0..n.div_ceil(self.tabwidth) {
                self.output.push(b'\t');
            }
            return;
        }
        for _ in 0..cellw - textw {
            self.output.push(b' ');
        }
    }

    fn write_lines(&mut self, pos0: usize, line0: usize, line1: usize) -> usize {
        let mut pos = pos0;
        for i in line0..line1 {
            let mut use_tabs = self.tab_indent;
            for j in 0..self.lines[i].len() {
                let c = self.lines[i][j];
                if c.size == 0 {
                    if j < self.widths.len() {
                        self.write_padding(c.width, self.widths[j], use_tabs);
                    }
                } else {
                    use_tabs = false;
                    self.output.extend_from_slice(&self.buf[pos..pos + c.size]);
                    pos += c.size;
                    if j < self.widths.len() {
                        self.write_padding(c.width, self.widths[j], false);
                    }
                }
            }
            if i + 1 == self.lines.len() {
                self.output
                    .extend_from_slice(&self.buf[pos..pos + self.cell.size]);
                pos += self.cell.size;
            } else {
                self.output.push(b'\n');
            }
        }
        pos
    }

    fn format(&mut self, pos0: usize, line0: usize, line1: usize) -> usize {
        let mut pos = pos0;
        let mut line0 = line0;
        let column = self.widths.len();
        let mut this = line0;
        while this < line1 {
            if column + 1 >= self.lines[this].len() {
                this += 1;
                continue;
            }
            pos = self.write_lines(pos, line0, this);
            line0 = this;
            let mut width = 0;
            let mut discardable = true;
            while this < line1 {
                let line = &self.lines[this];
                if column + 1 >= line.len() {
                    break;
                }
                let c = line[column];
                width = width.max(c.width + 1);
                if c.width > 0 || c.htab {
                    discardable = false;
                }
                this += 1;
            }
            if discardable {
                width = 0;
            }
            self.widths.push(width);
            pos = self.format(pos, line0, this);
            self.widths.pop();
            line0 = this;
        }
        self.write_lines(pos, line0, line1)
    }

    fn append(&mut self, text: &[u8]) {
        self.buf.extend_from_slice(text);
        self.cell.size += text.len();
    }

    fn update_width(&mut self) {
        self.cell.width += rune_count(&self.buf[self.pos..]);
        self.pos = self.buf.len();
    }

    fn end_escape(&mut self) {
        self.update_width();
        self.pos = self.buf.len();
        self.escaped = false;
    }

    fn terminate_cell(&mut self, htab: bool) -> usize {
        self.cell.htab = htab;
        let cell = self.cell;
        self.cell = Cell::default();
        let Some(line) = self.lines.last_mut() else {
            unreachable!("tabwriter always has a current line");
        };
        line.push(cell);
        line.len()
    }

    fn flush(&mut self) {
        if self.cell.size > 0 {
            if self.escaped {
                self.end_escape();
            }
            self.terminate_cell(false);
        }
        let len = self.lines.len();
        self.format(0, 0, len);
        self.reset();
    }

    fn write(&mut self, buf: &[u8]) {
        let mut n = 0;
        for (i, &ch) in buf.iter().enumerate() {
            if !self.escaped {
                match ch {
                    b'\t' | b'\x0b' | b'\n' | b'\x0c' => {
                        self.append(&buf[n..i]);
                        self.update_width();
                        n = i + 1;
                        let ncells = self.terminate_cell(ch == b'\t');
                        if ch == b'\n' || ch == b'\x0c' {
                            self.lines.push(Vec::new());
                            if ch == b'\x0c' || ncells == 1 {
                                self.flush();
                            }
                        }
                    }
                    ESCAPE => {
                        self.append(&buf[n..i]);
                        self.update_width();
                        n = i + 1;
                        self.escaped = true;
                    }
                    _ => {}
                }
            } else if ch == ESCAPE {
                self.append(&buf[n..i]);
                n = i + 1;
                self.end_escape();
            }
        }
        self.append(&buf[n..]);
    }
}
