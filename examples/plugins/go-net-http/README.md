# go-net-http

A **codegen** plugin: "copy as code" for Go, using only the standard
`net/http` package. The output is a complete program:

```go
package main

import (
	"fmt"
	"io"
	"log"
	"net/http"
)

func main() {
	req, err := http.NewRequest(http.MethodGet, "https://api.example.com/users", nil)
	if err != nil {
		log.Fatal(err)
	}
	req.Header.Add("Accept", "application/json")

	res, err := http.DefaultClient.Do(req)
	...
}
```

The request is the fully prepared one (variables expanded, auth applied), so
the generated code may contain secrets. No permissions are needed.

## Development

```bash
pt plugin validate
pt plugin test
```
