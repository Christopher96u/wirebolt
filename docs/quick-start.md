# Send your first request

[Documentation](README.md) · [Next: requests and responses](requests.md)

This example uses a small API running on your own Mac. It requires Python 3 and a checkout or download of this repository. It does not need an API account or credentials.

## Start the demo API

From the repository folder:

```sh
python3 docs/examples/demo-server.py
```

Leave that terminal open. It listens only on `127.0.0.1:18990`. Stop it with **Control-C** when finished. This is a documentation fixture, not a production server.

## Create and send a request

1. Open Wirebolt and choose **File → New Workspace…**. Choose a new folder.
2. Choose **File → New Collection…** and name it **Catalog API**.
3. Create an HTTP request from the toolbar **+** menu.
4. Choose **GET** and enter `http://127.0.0.1:18990/products`.
5. Click **Send** or press **⌘Return**.
6. Expect **200** and a JSON object containing `products` and `total`.
7. Press **⌘S**, choose the collection if prompted, and name the request **List products**.

If your system proxy interferes with loopback requests, select **Direct — No proxy** in the request's **Settings** tab, apply it and retry. This changes only that request.

## Send JSON

Create another request with method **POST** and URL `http://127.0.0.1:18990/echo`. In **Body**, select JSON and enter:

```json
{"name":"Desk lamp","quantity":2}
```

Send the request. The response includes your JSON under `body`. Save it as **Create order**.

![A JSON request and its successful response](assets/screenshots/overview.png)

## Reuse a base URL

Open the environment menu → **Configure Environments**. Add an enabled Global variable named `base_url` with value `http://127.0.0.1:18990`, then click **Save**. Change the request URL to `{{base_url}}/products` and send again.

Add your endpoint's usage notes under **Note → Edit**, then open **Preview** and save with **⌘S**.

Next, learn about [environments](environments-and-secrets.md), [authentication](authentication.md) or [proxy settings](proxy-and-network.md).
