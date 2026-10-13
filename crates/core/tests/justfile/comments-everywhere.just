# header

# before set
set shell := ["bash", "-c"] # after set
# before var
a := "1" # after var
export b := "2"   # after export
alias c := d # after alias
# before recipe
d: # after recipe header
  echo # inside body
# after body

# end
