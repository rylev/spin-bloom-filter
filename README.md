# Spin Bloom Filter

An example implementation of a bloom filter in [Spin](https://github.com/fermyon/spin).

This example presents two API endpoints:
* "POST /email" adds an email to the emails database
* "GET  /email" checks whether an email is present in the database

By using a bloom filter, the GET endpoint is able to more efficiently return a 200 OK
(the response when the email is not yet in the database - i.e., the more common response).

## Building

To build, you must have `spin` installed.

```bash
$ spin build --up
```

## Example

Check that the email is available:

```bash
$ curl -i http://127.0.01:3000/email\?email\=me@example.com
HTTP/1.1 200 OK
content-length: 0
```

Add the email to the database:

```bash
$ curl -i -XPOST -d '{"email": "me@example.com"}' http://127.0.01:3000/email
HTTP/1.1 200 OK
content-length: 0
```

Check that the email is not available

```bash
$ curl -i http://127.0.01:3000/email\?email\=me@example.com
HTTP/1.1 409 Conflict
content-length: 0
```

## What is a bloom filter?

A [bloom filter](https://en.wikipedia.org/wiki/Bloom_filter) is a way to efficiently (both compute and memory wise) tell whether something
is *not* present in a set.
