set positional-arguments

foo *bar:
  echo $1
  echo "$@"
