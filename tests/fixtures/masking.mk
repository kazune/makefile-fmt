foo.o:
	$(CC)  $(CFLAGS) -c $< -o $@
	echo   "$$HOME"
	echo   "$(NAME)"
	cp   $(SRC) $(DST)
	$(CXX)  $(CPPFLAGS) $(CXXFLAGS) -c $< -o $@
	echo  pre$(subst a,b,$(NAME))post '${NAME}' ${VAR_$(KEY)}
	@-+echo  $@ $< $^ $? $* $%
	@echo  __MAKEFMT_0_0__ $(NAME) $(NAME)
	@echo  '$$$$' "$$$$" \$$HOME
	@if test -n "$(NAME)"; then \
	echo "$$HOME"; \
	fi
