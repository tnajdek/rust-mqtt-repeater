rust-mqtt-repeater
----

Subscribes to selected topics at one MQTT broker and publishes similar events to another broker. Connection details and topics are read from a config file.


config
----
By default rust-mqtt-repeater will attempt to read `config.json` from current working directory, this behaviour can be overriden with a switch, e.g.

````bash
rust-mqtt-repeater -c /etc/rust-mqtt-repeater.json
````

Config file must be a JSON object with the following three top-level keys: `source`, `destination` and `topics`. First two define connection details for MQTT brokers, the following keys are recognized:

| Parameter    | Description                                                                       | Default            |
|--------------|-----------------------------------------------------------------------------------|--------------------|
| clientID     | Unique identifier to present to the broker                                        | rust-mqtt-repeater |
| host*        | Host name of the broker to connect                                                | -                  |
| port         | Port number of the broker to connect                                              | 8883               |
| keepAlive    | Time interval before PINGREQ is sent if no data flows through the open connection | 30                 |
| cleanSession | Whether to start a "clean session" (aka "non persistent connection")              | true               |
| connTimeout  | Connection timeout (in seconds)                                                   | 5                  |
| inflight     | Number of concurrent in flight messages                                           | 100                |
| auth*        | Authentication object, see below                                                  | -                  |


**Authentication object**

To authenticate with credentials, `auth` key should contain `login` and `password` keys. To authenticate using a certificate, `auth` should contain the following keys:

| Parameter   | Description                       | Default |
|-------------|-----------------------------------|---------|
| keyType     | Either "RSA" or "ECC"             | RSA     |
| ca*         | Certificate Authority certificate | -       |
| clientCert* | Certificate for this device       | -       |
| clientKey*  | Private key for this device       | -       |

**topics**

Topics should be an array of objects, where each object has the following keys: `to`, `from` to select topics to subscibe to at source and publish to at destination and optionally `payload` to define how to treat payload. If present, `payload` must be an object with a single key, one of the following: `behaviour`, `string` (value must be string), `bytes` (value must be array of bytes). If key is `behaviour`, it must have one of the following values:

| Parameter     | Description                                                                             |
|---------------|-----------------------------------------------------------------------------------------|
| copy          | Copy payload from source to destination                                                 |
| omit          | Always publish empty payload                                                            |
| invertBoolean | Attempt to "flip" a boolean value. Payloads "true"/"false" and "1"/"0" are recognized.  |


**Example**

See sample-config.json for example configuration.