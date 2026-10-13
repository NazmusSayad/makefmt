set lazy

x := 'foo'
y := 'bar'
z := `exit 1`

a:
  @echo {{x}}

b:
  @echo {{y}}
