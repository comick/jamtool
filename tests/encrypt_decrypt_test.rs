#[test]
fn test_jam_encryption_idempotency() {
    let original = b"Hello GP2 JAM File!".to_vec();
    let mut encrypted = original.clone();

    // Applying the rolling XOR twice should return the original data
    jamtool::decrypt_encrypt_jam(&mut encrypted);
    assert_ne!(original, encrypted);

    jamtool::decrypt_encrypt_jam(&mut encrypted);
    assert_eq!(original, encrypted);
}
