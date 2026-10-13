a := "x"
foo b:
  echo {{ if a == "x" { b } else { a } }}
  echo {{ uppercase(a) + b / "c" }}
