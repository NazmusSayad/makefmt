set working-directory := 'bar'

file := `cat file.txt`

@foo:
  echo {{ file }}
