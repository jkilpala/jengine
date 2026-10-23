//! `jengine-online` writes `jengine-net`'s `Net` channel by field name,
//! without linking the crate. A rename on either side would compile, run,
//! and silently stop moving any message - so the names are pinned here, the
//! one place both crates are in scope.

#[test]
fn the_net_channel_field_names_agree() {
    use jengine_net::channel;
    use jengine_online::net;
    assert_eq!(net::COMPONENT, channel::COMPONENT);
    assert_eq!(net::URL, channel::FIELD_URL);
    assert_eq!(net::OUTBOX, channel::FIELD_OUTBOX);
    assert_eq!(net::INBOX, channel::FIELD_INBOX);
    assert_eq!(net::STATE, channel::FIELD_STATE);
    assert_eq!(net::OWN, channel::FIELD_OWN);
}
