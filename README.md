## Ginger Connector

Its a CLI tool for

1. Discovering / managing services dependencies for Microservices / Full stack apps
2. Discovering and adding in house dependencies such as libraries ( Planned )


### Usage

```sh
ginger-connector init {__repo__}
```
This will ask for language and root_dir where Service clients will be generated


```sh
ginger-connector config
```

This will give you multi select choices of the service dependencies, already added services are selected by default. Check / Uncheck based on the requirement and then ENTER

```sh
ginger-connector dev
```

This will generate clients for using dev environment swagger files.

### Hosting service repository

1. Create a github repo say `your_company/services-repository`
2. Add __metadata__.json at the root and the following component

```json

{
  "services": [
    "ServiceOne",
    "ServiceTwo"
  ],
  "version": "0.4.0-nightly.0"
}

```
Now you must have `ServiceOne` folder in this repo with `dev.json` , `stage.json` and `prod.json`. These are swagger file of the respective environments.

Question : Why cant we use the live URL of these swagger file, why it needs to be available in this repo as JSON. 

Ans: We feel this is a better and error free mechanism. Your microservice should generate its swagger statically and update this repo. We are working on a tool/process to update.

## Notes

Build the binary for amazonlinux using the following commands

```sh

docker build -t ginger-connector . -f build-scripts/Dockerfile.amazonlinux
docker create --name temp_container ginger-connector
docker cp temp_container:/tmp/ginger-connector ./bin/
docker rm temp_container

```