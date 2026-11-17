use gtk::prelude::*;

pub(super) fn update(
    buffer: &impl IsA<gtk::TextBuffer>,
    previous: &mut String,
    content: &str,
) -> bool {
    if previous.as_str() == content {
        return false;
    }

    let buffer = buffer.as_ref();
    if let Some(appended) = content.strip_prefix(previous.as_str()) {
        let selection = buffer.has_selection().then(|| {
            (
                buffer.iter_at_mark(&buffer.get_insert()).offset(),
                buffer.iter_at_mark(&buffer.selection_bound()).offset(),
            )
        });
        buffer.insert(&mut buffer.end_iter(), appended);
        if let Some((insert, bound)) = selection {
            buffer.select_range(
                &buffer.iter_at_offset(insert),
                &buffer.iter_at_offset(bound),
            );
        }
        previous.push_str(appended);
        return true;
    }

    let shared = shared_prefix(previous, content);
    let offset = previous[..shared].chars().count() as i32;
    buffer.delete(&mut buffer.iter_at_offset(offset), &mut buffer.end_iter());
    buffer.insert(&mut buffer.end_iter(), &content[shared..]);
    previous.truncate(shared);
    previous.push_str(&content[shared..]);
    true
}

pub(super) fn shared_prefix(previous: &str, content: &str) -> usize {
    let mut shared = previous
        .bytes()
        .zip(content.bytes())
        .take_while(|(left, right)| left == right)
        .count();
    while !content.is_char_boundary(shared) {
        shared -= 1;
    }
    shared
}
