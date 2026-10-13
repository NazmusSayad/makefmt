set lists
set unstable

[arg('a', long)]
[arg('b', short='x')]
[arg('c', long='see', short)]
[arg("d", pattern="[0-9]+", help="a number")]
[arg('e', long, value="v")]
[arg('f', long, flag)]
[arg('g', long, multiple, min='1', max='3')]
foo a b c d e f g:
  echo
