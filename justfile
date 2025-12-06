build_debug:
  cargo build

build_release:
  cargo build --release

build: build_release

test_debug test_name='' $RUST_LOG="debug":
    cargo nextest run --workspace --no-capture {{test_name}}

test_release test_name='':
    cargo nextest run --workspace --release {{test_name}}

test: test_release

prod:
  cargo run --release -- --duco-ip=192.168.1.39 --duco-host duco_56dfcf.local --mqtt-addr=192.168.1.13 --mqtt-base-topic home/ventilation

dev $RUST_LOG="debug" $D2M_MQTT_USER="iot" $D2M_MQTT_PASS="":
  cargo run --release -- -vv --duco-ip=192.168.1.39 --duco-host duco_56dfcf.local --mqtt-addr=192.168.1.13 --mqtt-base-topic dbg/home/ventilation

docker:
  docker build -t dirkvdb/duco2mqtt:latest -f docker/BuildDockerfile .

dockerup:
  docker push dirkvdb/duco2mqtt:latest
