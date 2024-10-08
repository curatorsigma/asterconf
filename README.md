# asterconf
`asterconf` is a small web-Frontend that allows users to dynamically set call forwards used by asterisk.

# How it works:
## High-Level
Asterisk will have a subroutine like the following in its `extensions.conf`
```conf
[subCallForward]
exten => start,1,NoOp()
; if the AGI call is unsuccessfull (service down or similar)
; this will ensure that all calls go through, even if asterconf is unreachable
same => n,Set(CALL_FORWARDED_TO=${ARG1})
; set the digest secret - define it in asterconfs config.yaml
same => n,Set(BLAZING_AGI_DIGEST_SECRET=NOT_THE_SECRET)
; this sets the CALL_FORWARDED_TO variable to the correct value
same => n,AGI(agi://asterconf.example.com/call_forward,${ARG1},${ARG2})
same => n,Return(${CALL_FORWARDED_TO})
```

and it will call this subroutine like so:
```conf
exten => 12341234,1,NoOp()
same => n,GoSub(subCallForward,start,1(12341234,our_context))
same => n,Dial(PJSIP/${CALL_FORWARDED_TO})
```

This has the following effect:
- `12341234` is dialed
- the `asterconf` AGI-server is queried, which number to call when `12341234` is dialed from within `our_context`
- `subCallForward` sets `CALL_FORWARDED_TO` accordingly
- The call is placed to the number defined via the frontend (`asterconf`)

## Under the Hood
We use a postgresql database which stores the call forwards.
Asterisk makes FastAGI calls to the host running `asterconf`.
`asterconf` queries the postgresql database and returns the result.
`asterconf` also acts as a Web-Server that allows end users to make changes to the database.
Users are authenticated via LDAP to `asterconf`. You will need a running LDAP server to use `asterconf`.

# How to get started:
## Setup postgres
- You will need a postgres instance with a users owning a database in it.
- Migrations WILL be run against this database, so I highly recommend you create a new database for `asterconf`.

## Setup LDAP
- You will need a user with search access to the subtree containing your users.
- You can also specify a search filter to find users (e.g. using memberOf to get members of some group).
- Only LDAPS is supported. Your server will need to offer LDAP over TLS and your host will need to trust its certificate.
    - I highly recommend you setup LDAPS with publically trusted certificates, which you can e.g. do with Let's encrypt and reverse-proxying via nginx.

## Setup asterconf
It is recommended to run `asterconf` via docker-compose:
```sh
git clone https://github.com/curatorsigma/asterconf
cd asterconf

mkdir -p /etc/asterconf
cp config.example.yaml /etc/asterconf/config.yaml
# now make sure to change the required configuration parameters

mkdir -p /etc/ssl/asterconf
# consider getting a properly signed cert from your internal or an external pki
openssl req -new -x509 -days 365 -nodes -text -out /etc/ssl/asterconf/asterconf.cert -keyout /etc/ssl/asterconf/asterconf.key -subj "/CN=asterconf.example.com"

docker compose up
```

You can also run the application directly, if you want (or want to integrate into some other IaC):
```sh
git clone https://github.com/curatorsigma/asterconf
cd asterconf
cargo build --release

mkdir -p /whatever/path/you/want/
cp ./target/release/asterconf /whatever/path/you/want/

mkdir -p /etc/asterconf
cp config.example.yaml /etc/asterconf/config.yaml
# now make sure to change the required configuration parameters

mkdir -p /etc/ssl/asterconf
# consider getting a properly signed cert from your internal or an external pki
openssl req -new -x509 -days 365 -nodes -text -out /etc/ssl/asterconf/asterconf.cert -keyout /etc/ssl/asterconf/asterconf.key -subj "/CN=asterconf.example.com"

# or run via systemd-unit file or whatever you want
/whatever/path/you/want/asterconf
```

## Make changes to asterisk config
- Make the required changes to your `/etc/asterisk/extensions.conf`, so that `asterconf` is called.
- You will need to set `BLAZING_AGI_DIGEST_SECRET`. Consider replicating the example above.

# asterconf does not do what you want?
`asterconf` is mostly a thin CRUD-App around [blazing_agi](https://github.com/curatorsigma/blazing_agi), which defines the AGI server functionality.
If your use case requires another setup (other DB, different functionality, ...) then you might want to write your own Service (and frontend) around `blazing_agi` which handles the basic AGI functionality (and is available via cargo).

# License
This project is licensed under MIT-0 (MIT No Attribution).
By contributing to this repositry, you agree that your code will be licensed as MIT-0.

For my rationale for using MIT-0 instead of another more common license, please see
https://copy.church/objections/attribution/#why-not-require-attribution .


