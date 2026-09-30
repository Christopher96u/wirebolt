# Seed the demo API, then fetch the result.
curl -X POST https://api.example.test/v1/items \
  -u demo-user:demo-password \
  -F 'file=@/tmp/demo/report.pdf;type=application/pdf' \
  -F 'title=Quarterly report'
curl -sSL -k -m 5 --max-redirs 3 -A 'Wirebolt-Test/1.0' -e https://app.example.test "api.example.test/v1/items?limit=5" | jq .
curl -G https://api.example.test/v1/search -d 'q=desk lamp' --data-urlencode 'tag=a&b' -I
curl -XPUT https://api.example.test/v1/items/7 -H'Content-Type: text/csv' --data-binary @items.csv --proxy http://proxy.example.test:3128
curl https://api.example.test/v1/form -d 'a=1' -d 'b=two words' --data-urlencode 'c=x&y'
