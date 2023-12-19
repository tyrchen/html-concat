BUILD = build
HTMLS = $(wildcard $(BUILD)/*.html)
PDFS = $(HTMLS:%.html=%.pdf)

run:
	@cargo run --release

start-server:
	@simple-http-server -p 8888

generate-pdf:
	@rm -f $(PDFS)
	@make $(PDFS)

$(PDFS): %.pdf: %.html
	@echo "Generating $@ from $<"
	@npx chrome-headless-render-pdf --pdf $@ --url http://localhost:8888/$< --display-header-footer --header-template ' ' --footer-template '<style type="text/css">.footer{font-size:12px;width:100%;text-align:center;color:#000;padding-left:0.65cm;}</style><div class="footer"><span class="pageNumber"></span> / <span class="totalPages"></span></div>'
