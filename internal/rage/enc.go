package rage

import (
	"bytes"
	"crypto/rand"
	"encoding/base64"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
)

// Envelope is the on-disk password-envelope representation.
type Envelope struct {
	Version    uint16
	Salt       [SaltLen]byte
	Nonce      [NonceLen]byte
	Ciphertext []byte
}

// Parse reads an envelope from the wire format and validates the magic.
func Parse(data []byte) (*Envelope, error) {
	if !bytes.HasPrefix(data, []byte(Magic)) {
		return nil, ErrNotRage
	}
	p := data[len(Magic):]
	if len(p) < 2+SaltLen+NonceLen+4 {
		return nil, errors.New("rage: 数据过短")
	}
	env := &Envelope{Version: binary.LittleEndian.Uint16(p[:2])}
	if env.Version != Version {
		return nil, fmt.Errorf("rage: 不支持的版本 %d", env.Version)
	}
	copy(env.Salt[:], p[2:2+SaltLen])
	copy(env.Nonce[:], p[2+SaltLen:2+SaltLen+NonceLen])
	ctLen := binary.LittleEndian.Uint32(p[2+SaltLen+NonceLen:])
	ctStart := 2 + SaltLen + NonceLen + 4
	if uint32(len(p)-ctStart) != ctLen {
		return nil, errors.New("rage: 密文长度不匹配")
	}
	env.Ciphertext = append([]byte(nil), p[ctStart:ctStart+int(ctLen)]...)
	return env, nil
}

// IsRage reports whether the data begins with the rage password magic.
func IsRage(data []byte) bool { return bytes.HasPrefix(data, []byte(Magic)) }

// Bytes serialises the envelope back into the wire format.
func (e *Envelope) Bytes() []byte {
	buf := make([]byte, 0, len(Magic)+2+SaltLen+NonceLen+4+len(e.Ciphertext))
	buf = append(buf, []byte(Magic)...)
	var head [2 + SaltLen + NonceLen + 4]byte
	binary.LittleEndian.PutUint16(head[0:2], e.Version)
	copy(head[2:2+SaltLen], e.Salt[:])
	copy(head[2+SaltLen:2+SaltLen+NonceLen], e.Nonce[:])
	binary.LittleEndian.PutUint32(head[2+SaltLen+NonceLen:], uint32(len(e.Ciphertext)))
	buf = append(buf, head[:]...)
	buf = append(buf, e.Ciphertext...)
	return buf
}

// EncryptPage encrypts plaintext with a password and returns the envelope bytes.
func EncryptPage(plaintext []byte, password string) ([]byte, error) {
	salt := make([]byte, SaltLen)
	if _, err := io.ReadFull(rand.Reader, salt); err != nil {
		return nil, err
	}
	key, err := DeriveKey([]byte(password), salt)
	if err != nil {
		return nil, err
	}
	raw, err := Encrypt(key, plaintext)
	if err != nil {
		return nil, err
	}
	env := &Envelope{Version: Version}
	copy(env.Salt[:], salt)
	copy(env.Nonce[:], raw[:NonceLen])
	env.Ciphertext = raw[NonceLen:]
	return env.Bytes(), nil
}

// DecryptPage reverses EncryptPage. Returns the plaintext on success.
func DecryptPage(data []byte, password string) ([]byte, error) {
	env, err := Parse(data)
	if err != nil {
		return nil, err
	}
	key, err := DeriveKey([]byte(password), env.Salt[:])
	if err != nil {
		return nil, err
	}
	return Decrypt(key, append(env.Nonce[:], env.Ciphertext...))
}

// EncryptPageB64 wraps EncryptPage with base64 — useful for embedding rage
// envelopes inside Markdown fenced blocks without line-ending surprises.
func EncryptPageB64(plaintext []byte, password string) (string, error) {
	raw, err := EncryptPage(plaintext, password)
	if err != nil {
		return "", err
	}
	return base64.StdEncoding.EncodeToString(raw), nil
}

// DecryptPageB64 decodes a base64 envelope then decrypts it.
func DecryptPageB64(s string, password string) ([]byte, error) {
	raw, err := base64.StdEncoding.DecodeString(s)
	if err != nil {
		return nil, ErrNotRage
	}
	return DecryptPage(raw, password)
}
