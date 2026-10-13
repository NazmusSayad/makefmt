set no-cd := true

file := `cat data.txt`

@foo:
  echo {{file}}
