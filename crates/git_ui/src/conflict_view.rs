use collections::{HashMap, HashSet};
use editor::{
    ConflictsOurs, ConflictsOursMarker, ConflictsOuter, ConflictsTheirs, ConflictsTheirsMarker,
    Editor, EditorEvent, ExcerptId, InlayId, MultiBuffer, RowHighlightOptions, ToPoint,
    display_map::{
        BlockContext, BlockPlacement, BlockProperties, BlockStyle, CustomBlockId, Inlay,
    },
};
use gpui::{
    App, AppContext as _, Context, Entity, Hsla, InteractiveElement as _, ParentElement as _,
    Subscription, WeakEntity,
};
use language::{Anchor, Buffer, BufferId};
use project::{
    ConflictRegion, ConflictSet, ConflictSetSnapshot, ConflictSetUpdate, InlayHint,
    git_store::{Repository, RepositorySnapshot},
};
use std::{ops::Range, sync::Arc};
use ui::{
    ActiveTheme, AnyElement, Element as _, FluentBuilder, StatefulInteractiveElement, Styled,
    StyledTypography as _, div, h_flex, rems,
};

use crate::commit_tooltip::{CommitDetails, CommitTooltip};

pub(crate) struct ConflictAddon {
    buffers: HashMap<BufferId, BufferConflicts>,
    next_inlay_id: usize,
}

impl ConflictAddon {
    pub(crate) fn conflict_set(&self, buffer_id: BufferId) -> Option<Entity<ConflictSet>> {
        self.buffers
            .get(&buffer_id)
            .map(|entry| entry.conflict_set.clone())
    }
}

struct BufferConflicts {
    block_ids: Vec<(Range<Anchor>, CustomBlockId)>,
    ours_inlay_ids: Vec<InlayId>,
    theirs_inlay_ids: Vec<InlayId>,
    conflict_set: Entity<ConflictSet>,
    _subscription: Subscription,
}

impl editor::Addon for ConflictAddon {
    fn to_any(&self) -> &dyn std::any::Any {
        self
    }

    fn to_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

pub fn register_editor(editor: &mut Editor, buffer: Entity<MultiBuffer>, cx: &mut Context<Editor>) {
    editor.register_addon(ConflictAddon {
        buffers: Default::default(),
        next_inlay_id: 0,
    });

    let buffers = buffer.read(cx).all_buffers().clone();
    for buffer in buffers {
        buffer_added(editor, buffer, cx);
    }

    cx.subscribe(&cx.entity(), |editor, _, event, cx| match event {
        EditorEvent::ExcerptsAdded { buffer, .. } => buffer_added(editor, buffer.clone(), cx),
        EditorEvent::ExcerptsExpanded { ids } => {
            let multibuffer = editor.buffer().read(cx).snapshot(cx);
            for excerpt_id in ids {
                let Some(buffer) = multibuffer.buffer_for_excerpt(*excerpt_id) else {
                    continue;
                };
                let addon = editor.addon::<ConflictAddon>().unwrap();
                let Some(conflict_set) = addon.conflict_set(buffer.remote_id()).clone() else {
                    return;
                };
                excerpt_for_buffer_updated(editor, conflict_set, cx);
            }
        }
        EditorEvent::ExcerptsRemoved {
            removed_buffer_ids, ..
        } => buffers_removed(editor, removed_buffer_ids, cx),
        _ => {}
    })
    .detach();
}

fn excerpt_for_buffer_updated(
    editor: &mut Editor,
    conflict_set: Entity<ConflictSet>,
    cx: &mut Context<Editor>,
) {
    let conflicts_len = conflict_set.read(cx).snapshot().conflicts.len();
    conflicts_updated(
        editor,
        conflict_set,
        &ConflictSetUpdate {
            buffer_range: None,
            old_range: 0..conflicts_len,
            new_range: 0..conflicts_len,
        },
        cx,
    );
}

fn buffer_added(editor: &mut Editor, buffer: Entity<Buffer>, cx: &mut Context<Editor>) {
    let Some(project) = &editor.project else {
        return;
    };
    let git_store = project.read(cx).git_store().clone();

    let buffer_conflicts = editor
        .addon_mut::<ConflictAddon>()
        .unwrap()
        .buffers
        .entry(buffer.read(cx).remote_id())
        .or_insert_with(|| {
            let conflict_set = git_store.update(cx, |git_store, cx| {
                git_store.open_conflict_set(buffer.clone(), cx)
            });
            let subscription = cx.subscribe(&conflict_set, conflicts_updated);
            BufferConflicts {
                block_ids: Vec::new(),
                ours_inlay_ids: Vec::new(),
                theirs_inlay_ids: Vec::new(),
                conflict_set: conflict_set.clone(),
                _subscription: subscription,
            }
        });

    let conflict_set = buffer_conflicts.conflict_set.clone();
    let conflicts_len = conflict_set.read(cx).snapshot().conflicts.len();
    let addon_conflicts_len = buffer_conflicts.block_ids.len();
    conflicts_updated(
        editor,
        conflict_set,
        &ConflictSetUpdate {
            buffer_range: None,
            old_range: 0..addon_conflicts_len,
            new_range: 0..conflicts_len,
        },
        cx,
    );
}

fn buffers_removed(editor: &mut Editor, removed_buffer_ids: &[BufferId], cx: &mut Context<Editor>) {
    let mut removed_block_ids = HashSet::default();
    let mut removed_inlay_ids = Vec::new();
    editor
        .addon_mut::<ConflictAddon>()
        .unwrap()
        .buffers
        .retain(|buffer_id, buffer| {
            if removed_buffer_ids.contains(&buffer_id) {
                removed_block_ids.extend(buffer.block_ids.iter().map(|(_, block_id)| *block_id));
                removed_inlay_ids.extend(buffer.ours_inlay_ids.iter().copied());
                removed_inlay_ids.extend(buffer.theirs_inlay_ids.iter().copied());
                false
            } else {
                true
            }
        });
    editor.remove_blocks(removed_block_ids, None, cx);
    editor.splice_inlays(&removed_inlay_ids, Vec::new(), cx);
}

fn conflicts_updated(
    editor: &mut Editor,
    conflict_set: Entity<ConflictSet>,
    event: &ConflictSetUpdate,
    cx: &mut Context<Editor>,
) {
    let buffer_id = conflict_set.read(cx).snapshot.buffer_id;
    let conflict_set = conflict_set.read(cx).snapshot();
    let Some(repository) = editor.project.as_ref().and_then(|project| {
        let (repo, _) = project
            .read(cx)
            .git_store()
            .read(cx)
            .repository_and_path_for_buffer_id(conflict_set.buffer_id, cx)?;
        Some(repo)
    }) else {
        return;
    };
    let repository_snapshot = repository.read(cx).snapshot();
    let multibuffer = editor.buffer().read(cx);
    let snapshot = multibuffer.snapshot(cx);
    let excerpts = multibuffer.excerpts_for_buffer(buffer_id, cx);
    let Some(buffer_snapshot) = excerpts
        .first()
        .and_then(|(excerpt_id, _)| snapshot.buffer_for_excerpt(*excerpt_id))
    else {
        return;
    };

    // Remove obsolete highlights and blocks
    let conflict_addon = editor.addon_mut::<ConflictAddon>().unwrap();
    if let Some(buffer_conflicts) = conflict_addon.buffers.get_mut(&buffer_id) {
        let old_conflicts = buffer_conflicts.block_ids[event.old_range.clone()].to_owned();
        let mut removed_highlighted_ranges = Vec::new();
        let mut removed_block_ids = HashSet::default();
        for (conflict_range, block_id) in old_conflicts {
            let Some((excerpt_id, _)) = excerpts.iter().find(|(_, range)| {
                let precedes_start = range
                    .context
                    .start
                    .cmp(&conflict_range.start, &buffer_snapshot)
                    .is_le();
                let follows_end = range
                    .context
                    .end
                    .cmp(&conflict_range.start, &buffer_snapshot)
                    .is_ge();
                precedes_start && follows_end
            }) else {
                continue;
            };
            let excerpt_id = *excerpt_id;
            let Some(range) = snapshot
                .anchor_in_excerpt(excerpt_id, conflict_range.start)
                .zip(snapshot.anchor_in_excerpt(excerpt_id, conflict_range.end))
                .map(|(start, end)| start..end)
            else {
                continue;
            };
            removed_highlighted_ranges.push(range.clone());
            removed_block_ids.insert(block_id);
        }

        editor.remove_highlighted_rows::<ConflictsOuter>(removed_highlighted_ranges.clone(), cx);
        editor.remove_highlighted_rows::<ConflictsOurs>(removed_highlighted_ranges.clone(), cx);
        editor
            .remove_highlighted_rows::<ConflictsOursMarker>(removed_highlighted_ranges.clone(), cx);
        editor.remove_highlighted_rows::<ConflictsTheirs>(removed_highlighted_ranges.clone(), cx);
        editor.remove_highlighted_rows::<ConflictsTheirsMarker>(
            removed_highlighted_ranges.clone(),
            cx,
        );
        editor.remove_blocks(removed_block_ids, None, cx);
    }

    // Add new highlights and blocks
    let editor_handle = cx.weak_entity();
    let new_conflicts = &conflict_set.conflicts[event.new_range.clone()];
    let mut blocks = Vec::new();
    let mut ours_inlays = Vec::new();
    let mut theirs_inlays = Vec::new();
    for conflict in new_conflicts {
        let Some((excerpt_id, _)) = excerpts.iter().find(|(_, range)| {
            let precedes_start = range
                .context
                .start
                .cmp(&conflict.range.start, &buffer_snapshot)
                .is_le();
            let follows_end = range
                .context
                .end
                .cmp(&conflict.range.start, &buffer_snapshot)
                .is_ge();
            precedes_start && follows_end
        }) else {
            continue;
        };
        let excerpt_id = *excerpt_id;

        update_conflict_highlighting(editor, conflict, &snapshot, excerpt_id, cx);

        let Some(anchor) = snapshot.anchor_in_excerpt(excerpt_id, conflict.range.start) else {
            continue;
        };
        let Some(ours_inlay_anchor) =
            snapshot.anchor_in_excerpt(excerpt_id, conflict.ours_start_eol)
        else {
            continue;
        };
        let Some(theirs_inlay_anchor) =
            snapshot.anchor_in_excerpt(excerpt_id, conflict.theirs_end_eol)
        else {
            continue;
        };

        let editor_handle = editor_handle.clone();
        blocks.push(BlockProperties {
            placement: BlockPlacement::Above(anchor),
            height: Some(1),
            style: BlockStyle::Fixed,
            render: Arc::new({
                let conflict = conflict.clone();
                let conflict_set = conflict_set.clone();
                let repository = repository.clone();
                move |cx| {
                    render_conflict_buttons(
                        &conflict_set,
                        &repository,
                        &conflict,
                        excerpt_id,
                        editor_handle.clone(),
                        cx,
                    )
                }
            }),
            priority: 0,
        });
        let conflict_addon = editor.addon_mut::<ConflictAddon>().unwrap();
        ours_inlays.push(Inlay::conflict_marker(
            util::post_inc(&mut conflict_addon.next_inlay_id),
            ours_inlay_anchor,
            repository_snapshot.ours_name().to_owned(),
        ));
        theirs_inlays.push(Inlay::conflict_marker(
            util::post_inc(&mut conflict_addon.next_inlay_id),
            theirs_inlay_anchor,
            repository_snapshot.theirs_name().to_owned(),
        ));
    }
    let new_block_ids = editor.insert_blocks(blocks, None, cx);

    let conflict_addon = editor.addon_mut::<ConflictAddon>().unwrap();
    let (old_ours_inlays, old_theirs_inlays) =
        if let Some(buffer_conflicts) = conflict_addon.buffers.get_mut(&buffer_id) {
            buffer_conflicts.block_ids.splice(
                event.old_range.clone(),
                new_conflicts
                    .iter()
                    .map(|conflict| conflict.range.clone())
                    .zip(new_block_ids),
            );
            (
                buffer_conflicts
                    .ours_inlay_ids
                    .splice(
                        event.old_range.clone(),
                        ours_inlays.iter().map(|inlay| inlay.id),
                    )
                    .collect::<Vec<_>>(),
                buffer_conflicts
                    .theirs_inlay_ids
                    .splice(
                        event.old_range.clone(),
                        theirs_inlays.iter().map(|inlay| inlay.id),
                    )
                    .collect::<Vec<_>>(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
    editor.splice_inlays(&old_ours_inlays, ours_inlays, cx);
    editor.splice_inlays(&old_theirs_inlays, theirs_inlays, cx);
}

fn update_conflict_highlighting(
    editor: &mut Editor,
    conflict: &ConflictRegion,
    buffer: &editor::MultiBufferSnapshot,
    excerpt_id: editor::ExcerptId,
    cx: &mut Context<Editor>,
) {
    let theme = cx.theme().clone();
    let colors = theme.colors();
    let outer_start = buffer
        .anchor_in_excerpt(excerpt_id, conflict.range.start)
        .unwrap();
    let outer_end = buffer
        .anchor_in_excerpt(excerpt_id, conflict.range.end)
        .unwrap();
    let our_start = buffer
        .anchor_in_excerpt(excerpt_id, conflict.ours.start)
        .unwrap();
    let our_end = buffer
        .anchor_in_excerpt(excerpt_id, conflict.ours.end)
        .unwrap();
    let their_start = buffer
        .anchor_in_excerpt(excerpt_id, conflict.theirs.start)
        .unwrap();
    let their_end = buffer
        .anchor_in_excerpt(excerpt_id, conflict.theirs.end)
        .unwrap();

    let mut ours_background = colors.version_control_conflict_ours;
    let mut ours_marker = colors.version_control_conflict_ours;
    let mut theirs_marker = colors.version_control_conflict_theirs;
    let mut theirs_background = colors.version_control_conflict_theirs;
    ours_marker.fade_out(0.4);
    theirs_marker.fade_out(0.4);
    ours_background.fade_out(0.7);
    theirs_background.fade_out(0.7);

    let options = RowHighlightOptions {
        include_gutter: false,
        ..Default::default()
    };

    // Prevent diff hunk highlighting within the entire conflict region.
    editor.highlight_rows::<ConflictsOuter>(outer_start..outer_end, Hsla::default(), options, cx);
    editor.highlight_rows::<ConflictsOurs>(our_start..our_end, ours_background, options, cx);
    editor.highlight_rows::<ConflictsOursMarker>(outer_start..our_start, ours_marker, options, cx);
    editor.highlight_rows::<ConflictsTheirs>(
        their_start..their_end,
        theirs_background,
        options,
        cx,
    );
    editor.highlight_rows::<ConflictsTheirsMarker>(
        their_end..outer_end,
        theirs_marker,
        options,
        cx,
    );
}

fn render_conflict_buttons(
    conflict_set: &ConflictSetSnapshot,
    repository: &Entity<Repository>,
    conflict: &ConflictRegion,
    excerpt_id: ExcerptId,
    editor: WeakEntity<Editor>,
    cx: &mut BlockContext,
) -> AnyElement {
    let repo_snapshot = repository.read(cx).snapshot();
    let ours_name = format!("Take {}", repo_snapshot.ours_name());
    let theirs_name = format!("Take {}", repo_snapshot.theirs_name());
    let ours_details = repo_snapshot
        .head
        .as_ref()
        .and_then(|info| CommitDetails::parse(info).ok());
    let workspace = editor
        .upgrade()
        .and_then(|editor| editor.read(cx).workspace());
    h_flex()
        .h(cx.line_height)
        .items_end()
        .ml(cx.gutter_dimensions.width)
        .id(cx.block_id)
        .gap_0p5()
        .child(
            div()
                .id("ours")
                .px_1()
                .child(ours_name)
                .rounded_t(rems(0.2))
                .text_ui_sm(cx)
                .hover(|this| this.bg(cx.theme().colors().element_background))
                .cursor_pointer()
                .when_some(ours_details.zip(workspace), |el, (info, workspace)| {
                    let repository = repository.clone();
                    el.hoverable_tooltip(move |_window, cx| {
                        cx.new(|cx| {
                            CommitTooltip::new(
                                info.clone(),
                                repository.clone(),
                                workspace.downgrade(),
                                cx,
                            )
                        })
                        .into()
                    })
                })
                .on_click({
                    let editor = editor.clone();
                    let conflict = conflict.clone();
                    let ours = conflict.ours.clone();
                    move |_, _, cx| {
                        resolve_conflict(editor.clone(), excerpt_id, &conflict, &[ours.clone()], cx)
                    }
                }),
        )
        .child(
            div()
                .id("theirs")
                .px_1()
                .child(theirs_name)
                .rounded_t(rems(0.2))
                .text_ui_sm(cx)
                .hover(|this| this.bg(cx.theme().colors().element_background))
                .cursor_pointer()
                .on_click({
                    let editor = editor.clone();
                    let conflict = conflict.clone();
                    let theirs = conflict.theirs.clone();
                    move |_, _, cx| {
                        resolve_conflict(
                            editor.clone(),
                            excerpt_id,
                            &conflict,
                            &[theirs.clone()],
                            cx,
                        )
                    }
                }),
        )
        .child(
            div()
                .id("both")
                .px_1()
                .child("Take Both")
                .rounded_t(rems(0.2))
                .text_ui_sm(cx)
                .hover(|this| this.bg(cx.theme().colors().element_background))
                .cursor_pointer()
                .on_click({
                    let editor = editor.clone();
                    let conflict = conflict.clone();
                    let ours = conflict.ours.clone();
                    let theirs = conflict.theirs.clone();
                    move |_, _, cx| {
                        resolve_conflict(
                            editor.clone(),
                            excerpt_id,
                            &conflict,
                            &[ours.clone(), theirs.clone()],
                            cx,
                        )
                    }
                }),
        )
        .into_any()
}

fn resolve_conflict(
    editor: WeakEntity<Editor>,
    excerpt_id: ExcerptId,
    resolved_conflict: &ConflictRegion,
    ranges: &[Range<Anchor>],
    cx: &mut App,
) {
    let Some(editor) = editor.upgrade() else {
        return;
    };

    let multibuffer = editor.read(cx).buffer().read(cx);
    let snapshot = multibuffer.snapshot(cx);
    let Some(buffer) = resolved_conflict
        .ours
        .end
        .buffer_id
        .and_then(|buffer_id| multibuffer.buffer(buffer_id))
    else {
        return;
    };
    let buffer_snapshot = buffer.read(cx).snapshot();

    resolved_conflict.resolve(buffer, ranges, cx);

    editor.update(cx, |editor, cx| {
        let conflict_addon = editor.addon_mut::<ConflictAddon>().unwrap();
        let Some(state) = conflict_addon.buffers.get_mut(&buffer_snapshot.remote_id()) else {
            return;
        };
        let Ok(ix) = state.block_ids.binary_search_by(|(range, _)| {
            range
                .start
                .cmp(&resolved_conflict.range.start, &buffer_snapshot)
        }) else {
            return;
        };
        let &(_, block_id) = &state.block_ids[ix];
        let start = snapshot
            .anchor_in_excerpt(excerpt_id, resolved_conflict.range.start)
            .unwrap();
        let end = snapshot
            .anchor_in_excerpt(excerpt_id, resolved_conflict.range.end)
            .unwrap();
        editor.remove_highlighted_rows::<ConflictsOuter>(vec![start..end], cx);
        editor.remove_highlighted_rows::<ConflictsOurs>(vec![start..end], cx);
        editor.remove_highlighted_rows::<ConflictsTheirs>(vec![start..end], cx);
        editor.remove_highlighted_rows::<ConflictsOursMarker>(vec![start..end], cx);
        editor.remove_highlighted_rows::<ConflictsTheirsMarker>(vec![start..end], cx);
        editor.remove_blocks(HashSet::from_iter([block_id]), None, cx);
    })
}
