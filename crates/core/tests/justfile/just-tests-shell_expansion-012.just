foo a b:
  @echo {{a}}{{b}}
bar x b: (foo x 'c')
