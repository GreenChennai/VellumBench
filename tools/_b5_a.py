# -*- coding: utf-8 -*-
# B5:文档模型 P2 清账
IMP = r'crates\vb_doc\src\import.rs'
EXP = r'crates\vb_doc\src\export.rs'
MOD = r'crates\vb_doc\src\model.rs'
HTML = r'crates\vb_html\src\lib.rs'
CMDS = r'crates\vb_doc\src\commands.rs'

def rep(path, old, new, tag, count=1):
    s = open(path, encoding='utf-8').read()
    assert old in s, "anchor missing: " + tag
    open(path, 'w', encoding='utf-8', newline='').write(s.replace(old, new, count))

# ── 1) 连续注释保真:pending_comment → Vec,合并写入 comment_before ──
rep(IMP, "        pending_comment: None,", "        pending_comments: Vec::new(),", "init")
rep(IMP, "            NodeData::Comment(c) => importer.pending_comment = Some(c.clone()),",
            "            NodeData::Comment(c) => importer.pending_comments.push(c.clone()),", "outer-loop")
rep(IMP, "    pending_comment: Option<String>,", "    pending_comments: Vec<String>,", "field")
rep(IMP, """            NodeData::Comment(c) => {
                self.pending_comment = Some(c.clone());
            }""",
"""            NodeData::Comment(c) => {
                // 连续注释进队列,下一个节点全部带走(此前单槽后到覆盖,
                // `<!-- one --><!-- two -->` 只剩 two)
                self.pending_comments.push(c.clone());
            }""", "inner-comment")
rep(IMP, "        let comment = self.pending_comments.take();", "        let comment = self.take_pending_comments();", "take-616")
rep(IMP, "        let comment = self.pending_comment.take();", "        let comment = self.take_pending_comments();", "take-716")
rep(IMP, """    /// 把一个 body/画板子元素构建为节点并挂到 parent 下。""",
"""    /// 取走挂起的注释队列(合并为一条多行注释保内容;此前连续注释
    /// 只留最后一条)。空队列返回 None。
    fn take_pending_comments(&mut self) -> Option<String> {
        if self.pending_comments.is_empty() {
            return None;
        }
        Some(self.pending_comments.join("\\n"))
    }

    /// 把一个 body/画板子元素构建为节点并挂到 parent 下。""", "take-fn")

# ── 2) head 注释进 head_extra ──
rep(IMP, """    if let Some(head) = dom.head() {
        for child in &head.children {
            let Some(el) = child.as_element() else {
                continue;
            };""",
"""    if let Some(head) = dom.head() {
        for child in &head.children {
            // head 内注释保真(此前静默丢弃)
            if let NodeData::Comment(c) = &child.data {
                doc.head_extra.push(format!("<!--{c}-->"));
                continue;
            }
            let Some(el) = child.as_element() else {
                continue;
            };""", "head-comments")

# ── 3) html/body 属性保真:html 非 lang 属性、body 全属性 ──
rep(IMP, """    if let Some(html_el) = dom.root.as_element() {
        if let Some(l) = html_el.attr("lang") {
            lang = l.to_string();
        }
    }""",
"""    if let Some(html_el) = dom.root.as_element() {
        if let Some(l) = html_el.attr("lang") {
            lang = l.to_string();
        }
        // html/body 其余属性保真(此前 lang 之外全部丢失)
        for (k, v) in html_el.attrs.iter() {
            if k != "lang" && k != "data-vb-output" {
                doc.extra_html_attrs.push((k.clone(), v.clone()));
            }
        }
        if let Some(body_el) = dom.body().and_then(|b| b.as_element()) {
            for (k, v) in body_el.attrs.iter() {
                if k != "data-vb-output" {
                    doc.extra_body_attrs.push((k.clone(), v.clone()));
                }
            }
        }
    }""", "html-body-attrs")

print("B5 IMPORT PART PATCHED")
