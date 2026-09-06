$(OUTDIR):
	mkdir -p $(OUTDIR)

$(OUTDIR)/%: %.c | $(OUTDIR)
	$(CC) $(CFLAGS) $< -o $@ $(LDFLAGS) $(LDLIBS)

clean:
	rm -rf $(OUTDIR)
