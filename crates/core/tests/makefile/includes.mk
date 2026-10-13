include common.mk
-include missing.mk
sinclude   missing.mk   other.mk
-include	missing.mk # comment
include   $(wildcard common.mk)
