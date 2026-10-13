A=1
all:
	@echo   "kept"   >  out
	for f in a b; do \
	    echo $$f;  \
	done
