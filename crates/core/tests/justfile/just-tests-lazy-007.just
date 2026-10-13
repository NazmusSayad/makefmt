set lazy

c := `echo x >> count; wc -l < count | tr -d ' '`
f(y) := y + c

foo:
  @echo {{ f('') }}{{ f('') }}
