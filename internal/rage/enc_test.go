package rage

import (
	"bytes"
	"testing"
)

func TestEncryptDecryptRoundTrip(t *testing.T) {
	plain := []byte("# Hello, world!\n\nThis is a secret page.\n")
	pw := "correct horse battery staple"

	enc, err := EncryptPage(plain, pw)
	if err != nil {
		t.Fatalf("encrypt: %v", err)
	}
	if !IsRage(enc) {
		t.Fatal("isRage: 未识别加密数据")
	}

	got, err := DecryptPage(enc, pw)
	if err != nil {
		t.Fatalf("decrypt: %v", err)
	}
	if !bytes.Equal(got, plain) {
		t.Fatal("往返不一致")
	}
}

func TestDecryptWrongPassword(t *testing.T) {
	plain := []byte("top secret")
	enc, err := EncryptPage(plain, "right")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := DecryptPage(enc, "wrong"); err != ErrDecrypt {
		t.Fatalf("expected ErrDecrypt, got %v", err)
	}
}

func TestDecryptGarbage(t *testing.T) {
	if _, err := DecryptPage([]byte("not-rage!"), "x"); err != ErrNotRage {
		t.Fatalf("expected ErrNotRage, got %v", err)
	}
}

func TestEncryptDecryptB64(t *testing.T) {
	plain := []byte("行内 **markdown** 和 unicode：日本語 émoji ✅")
	pw := "密码123"

	s, err := EncryptPageB64(plain, pw)
	if err != nil {
		t.Fatal(err)
	}
	got, err := DecryptPageB64(s, pw)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, plain) {
		t.Fatal("base64 往返不一致")
	}
}

func TestGenerateKey(t *testing.T) {
	k1, _ := GenerateKey()
	k2, _ := GenerateKey()
	if len(k1) != KeyLen {
		t.Fatalf("key len = %d", len(k1))
	}
	if bytes.Equal(k1, k2) {
		t.Fatal("keys 不应该相同")
	}
}

func TestEncryptKeyRoundTrip(t *testing.T) {
	k, _ := GenerateKey()
	plain := []byte("server-side transparent encryption")

	raw, err := Encrypt(k, plain)
	if err != nil {
		t.Fatal(err)
	}
	got, err := Decrypt(k, raw)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, plain) {
		t.Fatal("key 往返不一致")
	}
}

func TestEnvelopeParse(t *testing.T) {
	plain := []byte("envelope parse test")
	pw := "p"

	enc, _ := EncryptPage(plain, pw)
	env, err := Parse(enc)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	if env.Version != Version {
		t.Fatalf("version = %d", env.Version)
	}

	// Bytes should be idempotent.
	again := env.Bytes()
	if !bytes.Equal(again, enc) {
		t.Fatal("Bytes() != original")
	}
}

func TestEmptyPage(t *testing.T) {
	// Empty-but-non-nil plaintext should encrypt and decrypt fine.
	enc, err := EncryptPage([]byte{}, "x")
	if err != nil {
		t.Fatal(err)
	}
	got, err := DecryptPage(enc, "x")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, []byte{}) {
		t.Fatal("空页面往返不一致")
	}
}

func TestLargePage(t *testing.T) {
	// 1 MiB of content — scrypt cost is fixed, box overhead small.
	plain := make([]byte, 1<<20)
	for i := range plain {
		plain[i] = byte(i * 37)
	}
	enc, err := EncryptPage(plain, "pw")
	if err != nil {
		t.Fatal(err)
	}
	got, err := DecryptPage(enc, "pw")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, plain) {
		t.Fatal("大页面往返不一致")
	}
}
