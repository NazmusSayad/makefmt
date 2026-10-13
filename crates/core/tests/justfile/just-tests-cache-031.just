[cache(outputs = 'foo')]
[working-directory('sub')]
[script]
bar:
  echo bar
  touch foo
