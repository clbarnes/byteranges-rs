ranges = bytes=-100 bytes=50-100 bytes=3000-
lorem = https://raw.githubusercontent.com/clbarnes/byteranges-rs/main/data/lorem.txt

.PHONY: all
all: $(ranges)

.PHONY: $(ranges)
$(ranges):
	curl $(lorem) -i -H "Range: $@" > data/response/$@.http1 2> /dev/null
