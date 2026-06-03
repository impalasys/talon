"use client";

import React from "react";
import { Streamdown } from "streamdown";

function border(color: string) {
  return `1px solid ${color}`;
}

export function MarkdownMessage({ children }: { children: string }) {
  const compactListChildren = (content: React.ReactNode): React.ReactNode =>
    React.Children.map(content, (child) => {
      if (!React.isValidElement(child)) {
        return child;
      }

      const elementChild = child as React.ReactElement<any>;
      const nextChildren = elementChild.props?.children
        ? compactListChildren(elementChild.props.children)
        : elementChild.props?.children;

      if (child.type === "p") {
        const paragraphChild = child as React.ReactElement<{
          style?: React.CSSProperties;
          children?: React.ReactNode;
        }>;
        return React.createElement("span", {
          style: {
            ...(paragraphChild.props.style || {}),
            margin: 0,
            display: "inline",
          },
          children: nextChildren,
        });
      }

      if (child.type === "br") {
        return null;
      }

      if (child.type === "ul" || child.type === "ol") {
        return React.cloneElement(child as React.ReactElement<any>, {
          style: {
            ...(elementChild.props.style || {}),
            marginTop: 0,
            marginBottom: "0.5rem",
            paddingLeft: "1.05rem",
          },
          children: nextChildren,
        });
      }

      if (typeof child.type === "string") {
        const nextStyle =
          child.type === "li"
            ? {
                ...(elementChild.props.style || {}),
                marginTop: 0,
                marginBottom: "0.25rem",
                lineHeight: 1.5,
              }
            : elementChild.props.style;

        return React.cloneElement(child as React.ReactElement<any>, {
          ...(nextStyle ? { style: nextStyle } : {}),
          ...(nextChildren !== undefined ? { children: nextChildren } : {}),
        });
      }

      return React.cloneElement(child as React.ReactElement<any>, {
        ...(nextChildren !== undefined ? { children: nextChildren } : {}),
      });
    });

  return (
    <div style={{ minWidth: 0, lineHeight: 1.6 }}>
      <Streamdown
        components={{
          p: (props) => <p {...props} style={{ margin: "0 0 0.45rem" }} />,
          ul: (props) => (
            <ul
              {...props}
              style={{ margin: "0.25rem 0 0.45rem", paddingLeft: "1.05rem", lineHeight: 1.5 }}
            />
          ),
          ol: (props) => (
            <ol
              {...props}
              style={{ margin: "0.25rem 0 0.45rem", paddingLeft: "1.05rem", lineHeight: 1.5 }}
            />
          ),
          li: (props) => (
            <li {...props} style={{ margin: "0 0 0.25rem", paddingLeft: "0.08rem", lineHeight: 1.5 }}>
              {compactListChildren(props.children)}
            </li>
          ),
          h1: (props) => (
            <h1
              {...props}
              style={{ margin: "0.7rem 0 0.35rem", fontSize: "1.3em", fontWeight: 700, lineHeight: 1.3 }}
            />
          ),
          h2: (props) => (
            <h2
              {...props}
              style={{ margin: "0.6rem 0 0.3rem", fontSize: "1.18em", fontWeight: 700, lineHeight: 1.35 }}
            />
          ),
          h3: (props) => (
            <h3
              {...props}
              style={{ margin: "0.5rem 0 0.25rem", fontSize: "1.08em", fontWeight: 700, lineHeight: 1.35 }}
            />
          ),
          pre: (props) => (
            <pre
              {...props}
              style={{
                margin: "0.55rem 0 0.7rem",
                padding: "0.75rem",
                overflowX: "auto",
                borderRadius: 12,
                border: border("rgba(148,163,184,0.24)"),
                background: "rgba(148,163,184,0.08)",
              }}
            />
          ),
          code: (props) => (
            <code
              {...props}
              style={{
                fontFamily: "ui-monospace, SFMono-Regular, monospace",
                fontSize: "0.92em",
              }}
            />
          ),
          a: (props) => <a {...props} style={{ color: "inherit", textDecoration: "underline" }} />,
          blockquote: (props) => (
            <blockquote
              {...props}
              style={{
                margin: "0.5rem 0 0.65rem",
                paddingLeft: "0.65rem",
                borderLeft: border("rgba(148,163,184,0.4)"),
                opacity: 0.88,
              }}
            />
          ),
        }}
      >
        {children}
      </Streamdown>
    </div>
  );
}
