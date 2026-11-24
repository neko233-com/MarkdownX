// Package rage provides symmetric encryption for MarkdownX pages, using
// scrypt for password-based key derivation and nacl/secretbox for
// authenticated encryption.
//
// Two wire formats coexist:
//
//	Password envelope (used by mdx-enc and the desktop password-save menu)
//	  Layout: "!rage!" + version(2,LE) + salt(16) + nonce(24) + ctLen(4,LE) + ciphertext
//
//	Server key envelope (used by the collaboration server's transparent encryption)
//	  Layout: "!sk!!" + nonce(24) + ciphertext
//
// Both are indistinguishable from plaintext by anything other than the
// first few bytes, so detection is a single prefix check.
package rage

import (
	"bytes"
	"crypto/rand"
	"errors"
	"io"

	"golang.org/x/crypto/nacl/secretbox"
	"golang.org/x/crypto/scrypt"
)

// KeyLen is the required key length for secretbox.
const KeyLen = 32

// SaltLen is the scrypt salt length.
const SaltLen = 16

// NonceLen is the secretbox nonce length.
const NonceLen = 24

// Version is the rage password-envelope format version emitted by this package.
const Version = 1

// Magic identifies a password-derived ciphertext.
const Magic = "!rage!"

// ServerMagic identifies a server-key ciphertext. Same length as Magic so
// detection stays a single prefix check.
const ServerMagic = "!sk!!"

// ErrNotRage is returned when the input does not start with the rage magic.
var ErrNotRage = errors.New("rage: 不是加密格式")

// ErrDecrypt is returned when authentication fails or a derived key is wrong.
var ErrDecrypt = errors.New("rage: 解密失败（密码错误或数据损坏）")

// GenerateKey produces a random 32-byte key for server-side transparent
// encryption. Each call yields a fresh key suitable for use with EncryptServer.
func GenerateKey() ([]byte, error) {
	k := make([]byte, KeyLen)
	if _, err := io.ReadFull(rand.Reader, k); err != nil {
		return nil, err
	}
	return k, nil
}

// DeriveKey stretches a password into a 32-byte secretbox key using scrypt.
// The output is deterministic for the same (password, salt) pair.
//
// scrypt parameters: N=2^15, r=8, p=1 (~32 MiB memory, ~50ms on a laptop).
func DeriveKey(password, salt []byte) ([]byte, error) {
	return scrypt.Key(password, salt, 1<<15, 8, 1, KeyLen)
}

// Encrypt encrypts plaintext with a random nonce and returns
// nonce || ciphertext. Lowest-level helper; callers usually want EncryptPage
// (password envelope) or EncryptServer (server-key envelope).
func Encrypt(key []byte, plaintext []byte) ([]byte, error) {
	if len(key) != KeyLen {
		return nil, errors.New("rage: key 必须是 32 字节")
	}
	var nonce [NonceLen]byte
	if _, err := io.ReadFull(rand.Reader, nonce[:]); err != nil {
		return nil, err
	}
	var k [KeyLen]byte
	copy(k[:], key)
	// secretbox.Seal does NOT prepend the nonce; reserve it manually.
	out := make([]byte, NonceLen, NonceLen+len(plaintext)+secretbox.Overhead)
	copy(out, nonce[:])
	out = secretbox.Seal(out[:NonceLen], plaintext, &nonce, &k)
	return out, nil
}

// Decrypt reverses Encrypt. input must start with NonceLen bytes of nonce.
func Decrypt(key []byte, input []byte) ([]byte, error) {
	if len(key) != KeyLen {
		return nil, errors.New("rage: key 必须是 32 字节")
	}
	if len(input) < NonceLen+secretbox.Overhead {
		return nil, ErrDecrypt
	}
	var nonce [NonceLen]byte
	copy(nonce[:], input[:NonceLen])
	var k [KeyLen]byte
	copy(k[:], key)
	plain, ok := secretbox.Open(nil, input[NonceLen:], &nonce, &k)
	if !ok {
		return nil, ErrDecrypt
	}
	return plain, nil
}

// EncryptServer wraps Encrypt with ServerMagic so it can be detected without
// knowing in advance whether a file is encrypted. Layout: !sk!! || nonce || ct.
func EncryptServer(key []byte, plaintext []byte) ([]byte, error) {
	raw, err := Encrypt(key, plaintext)
	if err != nil {
		return nil, err
	}
	out := make([]byte, 0, len(ServerMagic)+len(raw))
	out = append(out, []byte(ServerMagic)...)
	out = append(out, raw...)
	return out, nil
}

// DecryptServer reverses EncryptServer.
func DecryptServer(key []byte, data []byte) ([]byte, error) {
	if !IsServerEncrypted(data) {
		return nil, ErrDecrypt
	}
	return Decrypt(key, data[len(ServerMagic):])
}

// IsServerEncrypted reports whether data starts with the server ciphertext magic.
func IsServerEncrypted(data []byte) bool {
	return bytes.HasPrefix(data, []byte(ServerMagic))
}
