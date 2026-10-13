set unstable

foo() := foo()

bar:
  echo {{foo()}}
