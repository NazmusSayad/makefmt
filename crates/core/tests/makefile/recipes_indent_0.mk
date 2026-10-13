all:
	for f in a b; do \
	  echo $$f; \
	done
	cmd --a \
	    --b
