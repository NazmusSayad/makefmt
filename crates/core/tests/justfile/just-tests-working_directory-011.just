set working-directory := 'bar'

file := shell('cat file.txt')

@foo:
  echo {{ file }}
