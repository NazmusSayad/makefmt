[script]
foo:
  echo a

[script("bash", "-eu")]
bar:
  echo b

[script('python3')]
baz:
  print(1)
