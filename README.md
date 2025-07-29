![rstr](assets/logo-dark.png#gh-dark-mode-only) ![rstr](assets/logo-light.png#gh-light-mode-only)

Overview
-

**rstr** is an HTTP P2P file relay for Windows and Linux.

It allows you to transfer files stored on one PC to another PC through an HTTP server using WebSockets. All files are transferred in chunks - if the transfer interrupts you can always continue it from where it stopped and rstr will ensure file's integrity.

Getting Started
-
**rstr** comes as 2 separate programs - client and server.

It operates on the file metadata which includes:
- File name
- File size
- Last modified time
- Full file hash
- Hashes of the chunks

The client program provides a GUI with two modes:
- Sender Mode
- Receiver Mode

While the client program is running it will always maintain a connection with the server.

Sender mode is used on the PC that has the files that need to be transmitted. First you need to generate the file metadata. To do so click on "Upload" or "Upload multiple", select the files and click "Ok". You can see the metadata generation progress in the left bottom corner. After it is finished, rstr will upload the metadata to the server.

Once the metadata is uploaded the files will be shown to the receiver.

Receier mode is used to download the files from the Sender. To download the files the Sender must first be connected to the server. If the Sender is not connected, rstr will not allow you to download the files and will notify you about it. Click on the download icon next to the file to start download process.

Setting up a Server
-

> [!Warning]
> Currently the server does not support TLS configuration which is why it is very recommended that it is set up behind a reverse proxy (e.g. Nginx). Make sure to proxy the WebSocket connection as it is used as the main communication protocol.

**rstr** server uses the following environment variables for configuration:
- `RSTR_SERVER_PORT` - the port which the server will bind to
- `RSTR_SERVER_DATA_DIR` - the directory for storing server's data
- `RSTR_RECEIVER_TOKEN` - the token that will be used to authenticate the client in the Receiver Mode
- `RSTR_SENDER_TOKEN` - the token that will be used to authenticate the client in the Sender Mode