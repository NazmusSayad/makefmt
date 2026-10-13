foo a b="default" c='raw' d=(a + b) $e $f="x" +g:
  echo {{a}}

bar *rest:
  echo {{rest}}

baz $ x  =  "1"   * $ y:
  echo

qux +$args:
  echo
